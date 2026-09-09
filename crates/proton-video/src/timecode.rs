//! Timecode, and the rate the file does not record.
//!
//! ## Drop frame drops numbers, not frames
//!
//! The one thing about timecode that is worth stating plainly, because every
//! bug in this area comes from getting it backwards: 29.97 fps video runs
//! slightly slower than 30 frames a second, so a counter that ticks 30 times a
//! second drifts ahead of the clock by about 3.6 seconds an hour. Drop-frame
//! counting fixes the *label*, not the picture — it skips the numbers `:00`
//! and `:01` at the top of every minute except every tenth minute, so that at
//! `01:00:00:00` the tape has been running an hour. No frame of video is
//! thrown away. There are exactly as many frames either way.
//!
//! Which is why the two counts need different arithmetic to turn a timecode
//! into a frame number, and why a drop-frame list read as non-drop is wrong by
//! a couple of frames a minute — visible as drift, silent as a bug.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How the timeline is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    /// Nominal frames per second — the number the count reaches before rolling
    /// over to the next second. 30 for 29.97 material, 24 for 24, and so on.
    pub fps: u32,
    /// Whether frame *numbers* are dropped to keep the label honest.
    pub drop: bool,
}

impl Rate {
    /// Drop-frame is only defined for the 30 and 60 families — it exists to
    /// correct the 1000/1001 pulldown and means nothing at 24 or 25. A file
    /// asking for it anywhere else is counted straight rather than refused,
    /// because the rest of the list is still perfectly readable.
    pub fn new(fps: u32, drop: bool) -> Rate {
        Rate {
            fps: fps.max(1),
            drop: drop && matches!(fps, 30 | 60),
        }
    }

    /// How many frame numbers are skipped at the top of a minute: two at 30,
    /// four at 60.
    fn dropped(&self) -> u64 {
        match self.drop {
            true => u64::from(self.fps) / 15,
            false => 0,
        }
    }

    /// For the statusline: `24 fps · non-drop`.
    pub fn label(&self) -> String {
        format!(
            "{} fps · {}",
            self.fps,
            match self.drop {
                true => "drop frame",
                false => "non-drop",
            }
        )
    }
}

impl Default for Rate {
    fn default() -> Rate {
        Rate::new(crate::ASSUMED_FPS, false)
    }
}

/// A parsed `HH:MM:SS:FF`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timecode {
    pub hours: u64,
    pub minutes: u64,
    pub seconds: u64,
    pub frames: u64,
}

impl Timecode {
    /// Parse `HH:MM:SS:FF`.
    ///
    /// The last separator is `;` or `.` in some tools' drop-frame output and
    /// `:` in CMX3600, which carries the fact on its `FCM:` line instead. All
    /// three are accepted on the way in; what is written back is whatever the
    /// person typed, so a file's own convention survives being edited.
    pub fn parse(text: &str) -> Result<Timecode, String> {
        let parts: Vec<&str> = text.split([':', ';', '.']).collect();
        if parts.len() != 4 || parts.iter().any(|part| part.len() != 2) {
            return Err(format!("{text:?} is not a timecode like 01:00:00:00"));
        }
        let mut out = [0u64; 4];
        for (slot, part) in out.iter_mut().zip(&parts) {
            *slot = part
                .parse()
                .map_err(|_| format!("{text:?} is not a timecode like 01:00:00:00"))?;
        }
        let [hours, minutes, seconds, frames] = out;
        if minutes > 59 || seconds > 59 {
            return Err(format!("{text:?} has more than 60 minutes or seconds"));
        }
        Ok(Timecode {
            hours,
            minutes,
            seconds,
            frames,
        })
    }

    /// Which frame this is, counting from `00:00:00:00`.
    ///
    /// Drop-frame subtracts the numbers that were never used: two for every
    /// minute, given back every tenth minute.
    pub fn to_frames(self, rate: Rate) -> u64 {
        let fps = u64::from(rate.fps);
        let straight =
            ((self.hours * 60 + self.minutes) * 60 + self.seconds) * fps + self.frames.min(fps - 1);
        let minutes = self.hours * 60 + self.minutes;
        straight - rate.dropped() * (minutes - minutes / 10)
    }

    /// The inverse, for showing a length that was only ever a count of frames.
    pub fn from_frames(count: u64, rate: Rate) -> Timecode {
        let fps = u64::from(rate.fps);
        let mut n = count;
        // Put the skipped numbers back before splitting into fields, so the
        // ordinary arithmetic below sees the labels rather than the frames.
        if rate.drop {
            let dropped = rate.dropped();
            let per_ten = fps * 600 - 9 * dropped;
            let per_one = fps * 60 - dropped;
            let tens = count / per_ten;
            let rest = count % per_ten;
            n += 9 * dropped * tens;
            // The first minute of each ten is the one that skips nothing, so
            // anything inside it is already labelled correctly.
            if rest >= dropped {
                n += dropped * ((rest - dropped) / per_one);
            }
        }
        Timecode {
            hours: n / (fps * 3600),
            minutes: n / (fps * 60) % 60,
            seconds: n / fps % 60,
            frames: n % fps,
        }
    }

    /// Milliseconds to a frame number, for putting a caption — which is timed
    /// in thousandths of a second — on a ruler counted in frames.
    pub fn frames_from_ms(ms: u64, rate: Rate) -> u64 {
        // Nominal, not 1000/1001: an SRT beside a 29.97 edit is authored
        // against the same wall clock the drop-frame count already tracks.
        ms * u64::from(rate.fps) / 1000
    }
}

impl std::fmt::Display for Timecode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}:{:02}",
            self.hours, self.minutes, self.seconds, self.frames
        )
    }
}
