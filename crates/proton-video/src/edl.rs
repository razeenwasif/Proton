//! Reading and writing a CMX3600 edit decision list.
//!
//! ## The shape of the file
//!
//! A couple of header lines, then one line per event, then comments about the
//! event above them:
//!
//! ```text
//! TITLE: LAUNCH SEQUENCE
//! FCM: NON-DROP FRAME
//!
//! 001  AX       V     C        00:00:00:00 00:00:12:10 00:00:00:00 00:00:12:10
//! * FROM CLIP NAME: launch_a.mov
//!
//! 002  BX       V     D    024 00:00:04:00 00:00:09:12 00:00:12:10 00:00:17:22
//! * FROM CLIP NAME: ground_cam.mp4
//! ```
//!
//! The columns are, in order: the event number, the reel the footage is on,
//! which channels it lands in, the transition, the transition's length in
//! frames when it has one, and then four timecodes — where the piece starts
//! and ends *in the source*, and where it starts and ends *on the finished
//! programme*. Those last four are the whole document. Everything a cut is,
//! is in them.
//!
//! ## What is parsed, and what is left alone
//!
//! Event lines and the `*` comments that name their clips. Everything else —
//! `FCM:` changes mid-list, `AUD` remaps, motion-effect lines, the private
//! comments every tool writes — is text this does not touch, which is the same
//! rule `proton-game` follows and for the same reason: the parts a format
//! grows over forty years are exactly the parts a careless writer destroys.
//!
//! ## The columns are a convention, not a grammar
//!
//! Every NLE aligns an EDL slightly differently and some exceed the eight
//! characters CMX allowed for a reel name. So fields are read as whitespace-
//! separated tokens rather than fixed columns, the four timecodes are taken
//! from the end of the line where they always are, and nothing is re-aligned
//! on the way out. An edit replaces the bytes one field occupies; the
//! whitespace around it is the file's own.

use std::ops::Range;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::timecode::{Rate, Timecode};

/// The rate to read a list at when nobody has said.
///
/// An EDL does not record its frame rate — see the crate docs. 24 is the
/// default because it is the one a cut delivered as a file is most often at,
/// but the number matters less than the fact that it is *shown*: a timecode
/// read at the wrong rate is wrong quietly.
pub const ASSUMED_FPS: u32 = 24;

/// An edit decision list, and the text it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    text: String,
    rate: Rate,
}

/// One event: a piece of one source, landing somewhere on the programme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    /// The event number as written, `001`. Not an index — a dissolve is two
    /// events sharing one number, which is how CMX writes an incoming and an
    /// outgoing side.
    pub event: String,
    /// Which tape or file the footage is on. `AX` means "auxiliary", the
    /// conventional stand-in for a source with no reel of its own.
    pub reel: String,
    /// The channels as written: `V`, `A`, `A2`, `AA`, `AA/V`.
    pub channel: String,
    /// `C` for a cut, `D` for a dissolve, `W` and a number for a wipe.
    pub transition: String,
    /// The transition's length in frames. Empty for a cut, which has none.
    pub duration: String,
    /// From `* FROM CLIP NAME:`, when the tool that wrote the list left one.
    pub name: String,
    /// From `* SOURCE FILE:`, likewise.
    pub source: String,
    pub source_in: String,
    pub source_out: String,
    pub record_in: String,
    pub record_out: String,
    /// The lanes this event lands in — more than one for `AA/V`, which is a
    /// picture and both channels of its sound in a single event.
    pub tracks: Vec<String>,
    /// `record_in` as a frame count, for laying the clip out on a ruler.
    pub start: u64,
    /// `record_out`, likewise. An EDL's out point is exclusive: the first
    /// frame *not* included, so this is also where the next event begins.
    pub end: u64,
    /// 1-based, for opening the list at this event in Codex.
    pub line: usize,
    /// Where each editable field's bytes are. Stale the moment anything writes
    /// the file, so it is never sent anywhere and is re-scanned after a write.
    #[serde(skip)]
    #[ts(skip)]
    at: Spans,
}

/// The bytes behind the fields an inspector may change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Spans {
    reel: Range<usize>,
    source_in: Range<usize>,
    source_out: Range<usize>,
    record_in: Range<usize>,
    record_out: Range<usize>,
    /// Absent until something writes a `* FROM CLIP NAME:` line.
    name: Option<Range<usize>>,
    /// Just past the event line's newline, where such a comment would go.
    after: usize,
}

impl Timeline {
    /// Read a list at a stated rate.
    ///
    /// The rate is an argument because the file does not carry one. `FCM:`
    /// carries whether the count drops frame numbers, and that is taken from
    /// the file rather than from the caller.
    pub fn parse(text: &str, fps: u32) -> Result<Timeline, String> {
        let looks_like = text.lines().any(|line| event_tokens(line, 0).is_some());
        if !looks_like {
            return Err("this file has no edit events in it".into());
        }
        let drop = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("FCM:"))
            .map(|mode| {
                let mode = mode.trim().to_ascii_uppercase();
                // `NON-DROP FRAME` contains `DROP`, which is the trap this
                // format sets for everyone who reads it once.
                mode.contains("DROP") && !mode.starts_with("NON")
            })
            .unwrap_or(false);
        Ok(Timeline {
            text: text.to_string(),
            rate: Rate::new(fps, drop),
        })
    }

    pub fn read(path: &Path, fps: u32) -> Result<Timeline, String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        Timeline::parse(&text, fps).map_err(|why| format!("{}: {why}", path.display()))
    }

    /// The file as it should be written: byte for byte what was read, apart
    /// from whatever was edited.
    pub fn to_text(&self) -> &str {
        &self.text
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, &self.text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// How the list is being counted. Shown on screen, because it is an
    /// assumption and a hidden assumption is a lie.
    pub fn rate(&self) -> Rate {
        self.rate
    }

    /// From the `TITLE:` line, when there is one.
    pub fn title(&self) -> String {
        self.text
            .lines()
            .find_map(|line| line.trim().strip_prefix("TITLE:"))
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// The events, in the order the file lists them.
    pub fn clips(&self) -> Vec<Clip> {
        let mut out: Vec<Clip> = Vec::new();
        let mut at = 0usize;
        for line in self.text.split_inclusive('\n') {
            let start = at;
            at += line.len();
            let trimmed = line.trim_end_matches(['\r', '\n']);

            // A `*` comment describes the event above it. Reaching one before
            // any event means the file opens with a comment, which is fine and
            // means there is nothing to attach it to.
            if trimmed.trim_start().starts_with('*') {
                if let Some(clip) = out.last_mut() {
                    if let Some(found) = after_label(trimmed, start, "FROM CLIP NAME:") {
                        clip.name = self.text[found.clone()].to_string();
                        clip.at.name = Some(found);
                    } else if let Some(found) = after_label(trimmed, start, "SOURCE FILE:") {
                        clip.source = self.text[found].to_string();
                    }
                }
                continue;
            }

            let Some(fields) = event_tokens(trimmed, start) else {
                continue;
            };
            let text = |span: &Range<usize>| self.text[span.clone()].to_string();
            let frames = |span: &Range<usize>| {
                Timecode::parse(&self.text[span.clone()])
                    .map(|tc| tc.to_frames(self.rate))
                    .unwrap_or(0)
            };
            out.push(Clip {
                event: text(&fields.event),
                reel: text(&fields.reel),
                tracks: tracks(&self.text[fields.channel.clone()]),
                channel: text(&fields.channel),
                transition: text(&fields.transition),
                duration: fields.duration.as_ref().map(text).unwrap_or_default(),
                name: String::new(),
                source: String::new(),
                source_in: text(&fields.source_in),
                source_out: text(&fields.source_out),
                record_in: text(&fields.record_in),
                record_out: text(&fields.record_out),
                start: frames(&fields.record_in),
                end: frames(&fields.record_out),
                line: self.text[..start].lines().count() + 1,
                at: Spans {
                    reel: fields.reel,
                    source_in: fields.source_in,
                    source_out: fields.source_out,
                    record_in: fields.record_in,
                    record_out: fields.record_out,
                    name: None,
                    after: at,
                },
            });
        }
        out
    }

    /// Where the programme ends: the last frame any event reaches.
    ///
    /// Not the last event's out point — an EDL is not required to be in record
    /// order, and a list assembled from several passes usually is not.
    pub fn length(&self) -> u64 {
        self.clips().iter().map(|clip| clip.end).max().unwrap_or(0)
    }

    /// Change one field of one event.
    ///
    /// Answers whether anything changed, so setting a value to what it already
    /// is does not write the file.
    pub fn set_field(&mut self, index: usize, field: &str, value: &str) -> Result<bool, String> {
        let clip = self
            .clips()
            .into_iter()
            .nth(index)
            .ok_or_else(|| format!("there is no event at row {index}"))?;

        let (span, was) = match field {
            "reel" => (clip.at.reel, clip.reel),
            "sourceIn" => (clip.at.source_in, clip.source_in),
            "sourceOut" => (clip.at.source_out, clip.source_out),
            "recordIn" => (clip.at.record_in, clip.record_in),
            "recordOut" => (clip.at.record_out, clip.record_out),
            "name" => return self.set_name(&clip, value),
            other => return Err(format!("{other:?} is not a field of an event")),
        };

        if matches!(field, "sourceIn" | "sourceOut" | "recordIn" | "recordOut") {
            Timecode::parse(value)?;
        } else if value.is_empty() || value.split_whitespace().count() != 1 {
            // A reel name with a space in it would swallow the next column and
            // turn the line into a different edit.
            return Err("a reel name is one word".into());
        }

        if was == value {
            return Ok(false);
        }
        self.text.replace_range(span, value);
        Ok(true)
    }

    /// The clip name lives in a comment, which may not be there yet.
    fn set_name(&mut self, clip: &Clip, value: &str) -> Result<bool, String> {
        if value.contains(['\n', '\r']) {
            return Err("a clip name is one line".into());
        }
        match clip.at.name.clone() {
            Some(span) => {
                if clip.name == value {
                    return Ok(false);
                }
                self.text.replace_range(span, value);
            }
            // Written directly under its event, which is where every tool that
            // writes one puts it, and the only place it means anything.
            None => {
                let at = clip.at.after.min(self.text.len());
                let line = format!("* FROM CLIP NAME: {value}\n");
                let line = match self.text[..at].ends_with('\n') || at == 0 {
                    true => line,
                    false => format!("\n{line}"),
                };
                self.text.insert_str(at, &line);
            }
        }
        Ok(true)
    }
}

/// Where each field of an event line is.
struct Fields {
    event: Range<usize>,
    reel: Range<usize>,
    channel: Range<usize>,
    transition: Range<usize>,
    duration: Option<Range<usize>>,
    source_in: Range<usize>,
    source_out: Range<usize>,
    record_in: Range<usize>,
    record_out: Range<usize>,
}

/// Split an event line into its fields, or answer that it is not one.
///
/// `at` is where the line begins in the file, so the ranges come out absolute
/// and can be spliced without being offset again by the caller.
fn event_tokens(line: &str, at: usize) -> Option<Fields> {
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut from: Option<usize> = None;
    for (i, c) in line.char_indices() {
        match (c.is_whitespace(), from) {
            (false, None) => from = Some(i),
            (true, Some(start)) => {
                spans.push(at + start..at + i);
                from = None;
            }
            _ => {}
        }
    }
    if let Some(start) = from {
        spans.push(at + start..at + line.len());
    }

    // Event number, reel, channel, transition, then four timecodes.
    if spans.len() < 8 {
        return None;
    }
    let word = |span: &Range<usize>| &line[span.start - at..span.end - at];
    if !word(&spans[0]).chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // The four timecodes are the last four columns, wherever the optional
    // transition duration and any trailing junk put them.
    let tc = spans.len() - 4;
    if spans[tc..]
        .iter()
        .any(|span| Timecode::parse(word(span)).is_err())
    {
        return None;
    }
    Some(Fields {
        event: spans[0].clone(),
        reel: spans[1].clone(),
        channel: spans[2].clone(),
        transition: spans[3].clone(),
        duration: (tc > 4).then(|| spans[4].clone()),
        source_in: spans[tc].clone(),
        source_out: spans[tc + 1].clone(),
        record_in: spans[tc + 2].clone(),
        record_out: spans[tc + 3].clone(),
    })
}

/// The text after a comment's label, as a span, when the comment carries it.
fn after_label(line: &str, at: usize, label: &str) -> Option<Range<usize>> {
    let found = line.to_ascii_uppercase().find(label)?;
    let rest = &line[found + label.len()..];
    let lead = rest.len() - rest.trim_start().len();
    let value = rest.trim();
    match value.is_empty() {
        true => None,
        false => {
            let from = at + found + label.len() + lead;
            Some(from..from + value.len())
        }
    }
}

/// Which lanes a channel column lands in.
///
/// CMX names a picture `V`, one channel of sound `A`, the second `A2`, both
/// together `AA`, and combinations with a slash. `B` is the old name for
/// picture cut with its first audio channel. Anything unrecognised is given a
/// lane under its own name rather than dropped — an event that exists belongs
/// somewhere visible.
fn tracks(channel: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in channel.split('/').filter(|part| !part.is_empty()) {
        let lanes: Vec<String> = match part.to_ascii_uppercase().as_str() {
            "V" => vec!["V1".into()],
            "A" => vec!["A1".into()],
            "AA" => vec!["A1".into(), "A2".into()],
            "B" => vec!["V1".into(), "A1".into()],
            "NONE" => vec![],
            other => vec![other.to_string()],
        };
        for lane in lanes {
            if !out.contains(&lane) {
                out.push(lane);
            }
        }
    }
    out
}
