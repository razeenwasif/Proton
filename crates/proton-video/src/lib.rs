//! Video mode's document: an edit decision list, and the captions beside it.
//!
//! ## Why an EDL
//!
//! The architecture lists Video's document as `.edl` plus a sidecar `.srt`,
//! and the reason is the load-bearing rule: every mode's document is a text
//! file that diffs in Git. A timeline is the hardest case for that rule and
//! therefore the one worth getting right — Premiere's `.prproj` is gzipped
//! XML, Resolve keeps its timelines in a database, Final Cut's bundle is a
//! directory. None of them is reviewable, and none of them can be edited by
//! writing a patch.
//!
//! CMX3600 is forty years old, plain text, one line per event, and every tool
//! in the industry can read it. It records what a cut *is* — which frames of
//! which source land where on the record tape — and nothing else. That is a
//! narrower document than a modern NLE project, and the narrowness is the
//! point: what it does hold, it holds in a form a person can read in a diff.
//!
//! ## What it does not record, and what this does about it
//!
//! An EDL does not say what frame rate it is counted at. `FCM:` says whether
//! the count drops frame *numbers*, which implies 29.97 when it does, but a
//! non-drop list may be 24, 25, 30 or 50 and the file will not tell you. Every
//! NLE asks the person importing it. So does this: [`Timeline::parse`] takes
//! the rate as an argument, [`Timeline::rate`] gives it back, and the mode
//! shows it on screen — because a timecode read at the wrong rate is wrong
//! quietly, and an assumption that is displayed is not a lie.
//!
//! ## Nothing is re-printed
//!
//! An EDL is column-aligned by convention and every tool aligns it slightly
//! differently. Re-emitting one from a model would reformat every line of a
//! file somebody else's software will open next — the same reasoning as
//! `proton-game`, arrived at for the same reason. So an edit replaces exactly
//! the bytes the thing it changes occupies, and the rest of the file is the
//! bytes that were read.
//!
//! ## What this is not
//!
//! It reads a cut. It does not decode a frame of video. The program monitor,
//! waveforms, thumbnails, clip durations read from the media itself and any
//! kind of render all need ffmpeg in a process of its own, which the process
//! model already accounts for and which is not installed on this machine.
//! None of it is faked here.

mod edl;
mod srt;
mod timecode;

pub use edl::{Clip, Timeline, ASSUMED_FPS};
pub use srt::{Caption, Captions};
pub use timecode::{Rate, Timecode};
