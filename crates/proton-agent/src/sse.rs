//! Server-sent events, the wire format the Messages API streams over.
//!
//! Two layers, deliberately separate. This one knows only about the SSE line
//! protocol — `event:`, `data:`, a blank line dispatches — and hands up
//! `(event, data)` pairs without caring what is in them. What those pairs
//! *mean* is [`crate::stream`]'s problem.
//!
//! Kept apart because they fail differently. A framing bug drops or merges
//! events; a semantics bug mis-assembles a message. Debugging one while the
//! other is in the same function is how a streaming client eats an afternoon.

use std::io::{self, BufRead};

/// One dispatched event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// The `event:` field. The API always sends one; the spec does not require
    /// it, and the default is `message`.
    pub name: String,
    /// The `data:` field, with multi-line values joined by newlines.
    pub data: String,
}

/// Reads SSE events off a byte stream.
pub struct Events<R: BufRead> {
    input: R,
    line: String,
}

impl<R: BufRead> Events<R> {
    pub fn new(input: R) -> Self {
        Self {
            input,
            line: String::new(),
        }
    }
}

impl<R: BufRead> Iterator for Events<R> {
    type Item = io::Result<Event>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut name = String::new();
        let mut data: Option<String> = None;

        loop {
            self.line.clear();
            match self.input.read_line(&mut self.line) {
                Ok(0) => {
                    // End of stream. A half-built event here means the
                    // connection dropped mid-message, which is worth
                    // surfacing rather than quietly returning nothing.
                    return data.map(|_| {
                        Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "stream ended part-way through an event",
                        ))
                    });
                }
                Ok(_) => {}
                Err(err) => return Some(Err(err)),
            }

            let line = self.line.trim_end_matches(['\r', '\n']);

            // A blank line dispatches whatever has been collected.
            if line.is_empty() {
                match data {
                    Some(data) => {
                        return Some(Ok(Event {
                            name: if name.is_empty() {
                                "message".into()
                            } else {
                                name
                            },
                            data,
                        }))
                    }
                    // Blank lines between events, and the keep-alive newlines
                    // the API sends on a slow turn. Not an event.
                    None => continue,
                }
            }

            // `:` starts a comment, which is how some proxies keep a
            // connection warm.
            if line.starts_with(':') {
                continue;
            }

            let (field, value) = match line.split_once(':') {
                // A leading space after the colon is part of the syntax, not
                // of the value.
                Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
                // A field with no colon is a field with an empty value.
                None => (line, ""),
            };

            match field {
                "event" => name = value.to_string(),
                "data" => match &mut data {
                    // Several `data:` lines in one event join with newlines —
                    // that is the spec, and JSON pretty-printed by a proxy
                    // arrives exactly this way.
                    Some(existing) => {
                        existing.push('\n');
                        existing.push_str(value);
                    }
                    None => data = Some(value.to_string()),
                },
                // `id` and `retry` are for reconnection, which we do not do:
                // a half-finished assistant turn cannot be resumed by asking
                // for the rest of it.
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn events(text: &str) -> Vec<Event> {
        Events::new(Cursor::new(text.as_bytes().to_vec()))
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn an_event_is_a_name_and_a_payload() {
        let got = events("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n");
        assert_eq!(
            got,
            vec![Event {
                name: "message_stop".into(),
                data: "{\"type\":\"message_stop\"}".into(),
            }]
        );
    }

    #[test]
    fn events_are_separated_by_blank_lines_not_counted() {
        let got = events(
            "event: a\ndata: 1\n\n\
             event: b\ndata: 2\n\n\
             event: c\ndata: 3\n\n",
        );
        assert_eq!(
            got.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn several_data_lines_join_with_newlines() {
        // The spec's rule, and what a proxy that re-wraps JSON produces.
        let got = events("event: x\ndata: {\ndata:   \"a\": 1\ndata: }\n\n");
        assert_eq!(got[0].data, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn comments_and_keepalives_are_not_events() {
        // Long turns are held open with comment lines and bare newlines. Every
        // one of them arriving as an empty event would be a redraw.
        let got = events(": keep-alive\n\n\n: another\nevent: real\ndata: {}\n\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "real");
    }

    #[test]
    fn only_one_space_after_the_colon_is_syntax() {
        // `data:  x` is the value " x". Trimming eagerly corrupts text deltas,
        // which is how leading spaces go missing from streamed prose.
        let got = events("event: x\ndata:  two spaces\n\n");
        assert_eq!(got[0].data, " two spaces");
    }

    #[test]
    fn an_event_with_no_name_defaults_to_message() {
        let got = events("data: bare\n\n");
        assert_eq!(got[0].name, "message");
    }

    #[test]
    fn a_stream_cut_mid_event_is_an_error_not_an_ending() {
        // The failure that matters: a dropped connection must not look like a
        // turn that finished.
        let mut it = Events::new(Cursor::new(b"event: x\ndata: {}".to_vec()));
        assert!(it.next().unwrap().is_err());
    }

    #[test]
    fn a_clean_end_between_events_is_just_the_end() {
        let mut it = Events::new(Cursor::new(b"event: x\ndata: {}\n\n".to_vec()));
        assert!(it.next().unwrap().is_ok());
        assert!(it.next().is_none());
    }
}
