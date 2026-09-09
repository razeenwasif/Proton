//! Reading and writing the caption sidecar.
//!
//! ## The shape of the file
//!
//! Cues separated by blank lines, each a number, a timing line, and the words:
//!
//! ```text
//! 1
//! 00:00:02,000 --> 00:00:05,500
//! ...and that is the burn complete.
//!
//! 2
//! 00:00:06,120 --> 00:00:09,000
//! Orbital insertion confirmed.
//! ```
//!
//! ## Why the captions are a separate file
//!
//! Because they already are. SRT is what a transcription tool emits, what a
//! player loads, and what every video site accepts — and the architecture
//! lists Video's document as an EDL *plus a sidecar* for that reason. Folding
//! the words into the cut would produce a file only this editor could read,
//! which is the mistake the whole diffable-document rule exists to prevent.
//!
//! ## Two clocks
//!
//! An EDL counts frames; SRT counts thousandths of a second. They are the same
//! wall clock read two ways, and turning one into the other needs the frame
//! rate the EDL does not record — which is why the rate is stated out loud in
//! [`crate::Timeline`] rather than assumed here. This file keeps its own times
//! exactly as written and lets the caller do the conversion knowingly.
//!
//! ## Nothing is re-printed
//!
//! Same rule as everywhere else: an edit replaces the bytes the thing it
//! changes occupies. A caption file is usually machine-written and re-writing
//! one wholesale would churn every line of a diff to change one word.

use std::ops::Range;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A caption file, and the text it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captions {
    text: String,
}

/// One cue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Caption {
    /// The cue number as written. Empty for a file that omits them, which is
    /// unusual but readable and so is not an error.
    pub index: String,
    pub start: String,
    pub end: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// The words. Several lines are joined with a newline, as in the file — a
    /// two-line caption is a deliberate choice about how it breaks on screen.
    pub text: String,
    /// 1-based, for opening the file at this cue in Codex.
    pub line: usize,
    #[serde(skip)]
    #[ts(skip)]
    at: CueSpans,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CueSpans {
    start: Range<usize>,
    end: Range<usize>,
    text: Range<usize>,
}

impl Captions {
    /// Parse a caption file. An empty one is a caption file with no cues in
    /// it, which is what a transcription that found no speech looks like.
    pub fn parse(text: &str) -> Result<Captions, String> {
        let looks_like = text.trim().is_empty() || text.contains("-->");
        match looks_like {
            true => Ok(Captions {
                text: text.to_string(),
            }),
            false => Err("this file has no captions in it".into()),
        }
    }

    pub fn read(path: &Path) -> Result<Captions, String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        Captions::parse(&text).map_err(|why| format!("{}: {why}", path.display()))
    }

    pub fn to_text(&self) -> &str {
        &self.text
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, &self.text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// The cues, in file order.
    pub fn captions(&self) -> Vec<Caption> {
        let mut out: Vec<Caption> = Vec::new();
        let lines = offsets(&self.text);
        let mut i = 0;
        while i < lines.len() {
            let (at, line) = lines[i];
            let Some(arrow) = line.find("-->") else {
                i += 1;
                continue;
            };
            // The number sits on the line above, when the file bothers with
            // one. A file that does not is still perfectly playable.
            let index = match i > 0 {
                true => lines[i - 1].1.trim().to_string(),
                false => String::new(),
            };
            let index = match index.chars().all(|c| c.is_ascii_digit()) && !index.is_empty() {
                true => index,
                false => String::new(),
            };

            let start = stamp(line, at, 0..arrow);
            // Anything after the second stamp is a positioning hint some tools
            // add. It is left where it is.
            let end = stamp(line, at, arrow + 3..line.len());
            let (Some(start), Some(end)) = (start, end) else {
                i += 1;
                continue;
            };

            // The words run to the first blank line, which is what ends a cue.
            let first = i + 1;
            let mut last = first;
            while last < lines.len() && !lines[last].1.trim().is_empty() {
                last += 1;
            }
            let span = match first < last {
                true => lines[first].0..lines[last - 1].0 + lines[last - 1].1.len(),
                // A cue with a timing line and no words. Its span is the
                // empty place where the words would go — the start of the
                // next line, not the end of this one, or typing some would
                // put them on the end of the timing line.
                false => {
                    let after = lines
                        .get(first)
                        .map(|(offset, _)| *offset)
                        .unwrap_or(self.text.len());
                    after..after
                }
            };

            out.push(Caption {
                index,
                start_ms: to_ms(&self.text[start.clone()]).unwrap_or(0),
                end_ms: to_ms(&self.text[end.clone()]).unwrap_or(0),
                start: self.text[start.clone()].to_string(),
                end: self.text[end.clone()].to_string(),
                text: self.text[span.clone()].to_string(),
                line: self.text[..at].lines().count() + 1,
                at: CueSpans {
                    start,
                    end,
                    text: span,
                },
            });
            i = last.max(i + 1);
        }
        out
    }

    /// Change one field of one cue: `text`, `start` or `end`.
    ///
    /// Answers whether anything changed, so setting a value to what it already
    /// is does not write the file.
    pub fn set_field(&mut self, index: usize, field: &str, value: &str) -> Result<bool, String> {
        let cue = self
            .captions()
            .into_iter()
            .nth(index)
            .ok_or_else(|| format!("there is no caption at row {index}"))?;

        let (span, was) = match field {
            "text" => {
                // A blank line is what ends a cue, so one inside the words
                // would silently split this caption in two.
                if value.lines().any(|line| line.trim().is_empty()) {
                    return Err("a blank line would end the caption".into());
                }
                (cue.at.text, cue.text)
            }
            "start" => {
                to_ms(value)?;
                (cue.at.start, cue.start)
            }
            "end" => {
                to_ms(value)?;
                (cue.at.end, cue.end)
            }
            other => return Err(format!("{other:?} is not a field of a caption")),
        };

        if was == value {
            return Ok(false);
        }
        // Words going into a cue that had none need a line of their own. The
        // blank line they are being written in front of is what separates this
        // cue from the next one, so writing into it without a newline of our
        // own would consume the separator and join two cues into one.
        if span.is_empty() {
            let mut line = value.to_string();
            // Unless the file ended on the timing line, in which case there is
            // no newline yet and the words need one before them instead.
            if span.start > 0 && !self.text[..span.start].ends_with('\n') {
                line.insert(0, '\n');
            }
            if span.start < self.text.len() {
                line.push('\n');
            }
            self.text.insert_str(span.start, &line);
            return Ok(true);
        }
        self.text.replace_range(span, value);
        Ok(true)
    }
}

/// Every line, with where it starts and without its newline.
fn offsets(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        out.push((at, line.trim_end_matches(['\r', '\n'])));
        at += line.len();
    }
    out
}

/// The one timestamp inside a slice of the timing line, as an absolute span.
fn stamp(line: &str, at: usize, within: Range<usize>) -> Option<Range<usize>> {
    let part = line.get(within.clone())?;
    let found = part.find(|c: char| c.is_ascii_digit())?;
    let rest = &part[found..];
    let len = rest
        .find(|c: char| !(c.is_ascii_digit() || c == ':' || c == ',' || c == '.'))
        .unwrap_or(rest.len());
    let text = &rest[..len];
    to_ms(text).ok()?;
    let from = at + within.start + found;
    Some(from..from + len)
}

/// `HH:MM:SS,mmm` in thousandths.
///
/// The comma is the format's own; a full stop is what tools written by people
/// who use one as a decimal point emit, and it is read the same way.
fn to_ms(text: &str) -> Result<u64, String> {
    let bad = || format!("{text:?} is not a caption time like 00:00:02,000");
    let (clock, millis) = text.rsplit_once([',', '.']).ok_or_else(bad)?;
    let parts: Vec<&str> = clock.split(':').collect();
    if parts.len() != 3 || millis.len() != 3 {
        return Err(bad());
    }
    let mut out = [0u64; 3];
    for (slot, part) in out.iter_mut().zip(&parts) {
        *slot = part.parse().map_err(|_| bad())?;
    }
    let [hours, minutes, seconds] = out;
    if minutes > 59 || seconds > 59 {
        return Err(format!("{text:?} has more than 60 minutes or seconds"));
    }
    Ok(
        ((hours * 60 + minutes) * 60 + seconds) * 1000
            + millis.parse::<u64>().map_err(|_| bad())?,
    )
}
