//! Video mode: the cut the timeline shows.
//!
//! `proton-video` owns the two files. What lives here is the editor's half —
//! which list is open, which caption file goes with it, which event the
//! timeline is on, and what happens to the files when somebody changes a
//! timecode or a line of dialogue.
//!
//! The same shape as [`crate::game`] and [`crate::modeling`], because it is
//! the same job a fourth time.
//!
//! ## The rate is a decision, and it is made here
//!
//! An EDL does not record the rate it is counted at. Every NLE asks on import
//! and so does this: the rate is held here, sent to the front end so it can be
//! shown, and changeable — because the only wrong thing to do with an
//! assumption is to hide it.
//!
//! ## What this cannot do
//!
//! Decode a frame. The media bin below lists the sources the cut refers to and
//! says which of them are actually on disk, which is a real and useful thing
//! to know and needs nothing but the filesystem. Thumbnails, waveforms,
//! durations read from the media itself, a program monitor and any kind of
//! render need ffmpeg in a process of its own. None of them is faked.

use std::path::{Path, PathBuf};

use proton_video::{Caption, Captions, Clip, Rate, Timecode, Timeline, ASSUMED_FPS};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One row of the media bin: a source the cut refers to.
///
/// Derived from the list rather than from a folder somebody pointed at,
/// because what a cut needs is exactly what it refers to — and a source it
/// refers to that is *not* there is the single most useful thing this panel
/// can tell anybody.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    /// The clip name the list gives it, or the reel when it gives no name.
    pub name: String,
    /// Where it was found, relative to the list. Empty when it was not.
    pub path: String,
    pub reel: String,
    /// How many events use it.
    pub uses: usize,
    /// Whether a file by that name is actually there.
    pub found: bool,
    /// Whether every event using it is sound only.
    pub audio: bool,
}

/// The cut that is open, and where the timeline is in it.
#[derive(Debug)]
pub struct Video {
    timeline: Option<Timeline>,
    path: Option<PathBuf>,
    captions: Option<Captions>,
    captions_path: Option<PathBuf>,
    /// `usize::MAX` for none.
    selected: usize,
    problem: Option<String>,
    fps: u32,
}

impl Default for Video {
    fn default() -> Video {
        Video {
            timeline: None,
            path: None,
            captions: None,
            captions_path: None,
            selected: usize::MAX,
            problem: None,
            fps: ASSUMED_FPS,
        }
    }
}

impl Video {
    pub fn is_open(&self) -> bool {
        self.timeline.is_some()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn captions_path(&self) -> Option<&Path> {
        self.captions_path.as_deref()
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn rate(&self) -> Rate {
        self.timeline
            .as_ref()
            .map(Timeline::rate)
            .unwrap_or_else(|| Rate::new(self.fps, false))
    }

    pub fn title(&self) -> String {
        self.timeline
            .as_ref()
            .map(Timeline::title)
            .unwrap_or_default()
    }

    /// The list's own text, for the pane that shows it.
    pub fn source(&self) -> String {
        self.timeline
            .as_ref()
            .map(|one| one.to_text().to_string())
            .unwrap_or_default()
    }

    pub fn clips(&self) -> Vec<Clip> {
        self.timeline
            .as_ref()
            .map(Timeline::clips)
            .unwrap_or_default()
    }

    pub fn captions(&self) -> Vec<Caption> {
        self.captions
            .as_ref()
            .map(Captions::captions)
            .unwrap_or_default()
    }

    /// Where the programme ends, in frames.
    pub fn length(&self) -> u64 {
        let cut = self.timeline.as_ref().map(Timeline::length).unwrap_or(0);
        // A caption may outlast the picture — a title card over black at the
        // end is exactly that — and a ruler that stopped at the last cut would
        // put it off the edge of the timeline.
        let said = self
            .captions()
            .iter()
            .map(|one| Timecode::frames_from_ms(one.end_ms, self.rate()))
            .max()
            .unwrap_or(0);
        cut.max(said)
    }

    /// The lanes, top to bottom, as an NLE stacks them: picture above, sound
    /// below, and only the ones this cut actually uses.
    pub fn tracks(&self) -> Vec<String> {
        let mut video: Vec<String> = Vec::new();
        let mut audio: Vec<String> = Vec::new();
        let mut other: Vec<String> = Vec::new();
        for lane in self.clips().into_iter().flat_map(|clip| clip.tracks) {
            let into = match lane.as_bytes().first() {
                Some(b'V') => &mut video,
                Some(b'A') => &mut audio,
                _ => &mut other,
            };
            if !into.contains(&lane) {
                into.push(lane);
            }
        }
        video.sort();
        video.reverse();
        audio.sort();
        other.sort();
        video.into_iter().chain(other).chain(audio).collect()
    }

    /// The media bin.
    pub fn media(&self) -> Vec<MediaItem> {
        let beside = self.path.as_ref().and_then(|path| path.parent());
        let mut out: Vec<MediaItem> = Vec::new();
        for clip in self.clips() {
            let name = match clip.name.is_empty() {
                true => clip.reel.clone(),
                false => clip.name.clone(),
            };
            if name.is_empty() {
                continue;
            }
            let sound_only = !clip.tracks.iter().any(|lane| lane.starts_with('V'));
            if let Some(already) = out.iter_mut().find(|one| one.name == name) {
                already.uses += 1;
                already.audio &= sound_only;
                continue;
            }
            // `* SOURCE FILE:` when the list gives one, otherwise the clip
            // name beside the list. Both are guesses about where the media
            // lives; what is not a guess is whether the file is there.
            let (path, found) = locate(beside, &clip.source, &name);
            out.push(MediaItem {
                name,
                path,
                found,
                reel: clip.reel,
                uses: 1,
                audio: sound_only,
            });
        }
        out
    }

    /// Open a list, and the caption file beside it.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        match Timeline::read(path, self.fps) {
            Ok(timeline) => {
                self.timeline = Some(timeline);
                self.path = Some(path.to_path_buf());
                self.problem = None;
                self.selected = usize::MAX;
                self.open_captions(path);
                Ok(())
            }
            Err(why) => {
                self.timeline = None;
                self.captions = None;
                self.captions_path = None;
                self.path = Some(path.to_path_buf());
                self.problem = Some(why.clone());
                self.selected = usize::MAX;
                Err(why)
            }
        }
    }

    /// The sidecar: the same name with `.srt`, or the only one in the folder.
    ///
    /// A missing caption file is not a problem to report. Plenty of cuts have
    /// no dialogue in them.
    fn open_captions(&mut self, path: &Path) {
        self.captions = None;
        self.captions_path = None;
        for candidate in sidecars(path) {
            if let Ok(found) = Captions::read(&candidate) {
                self.captions = Some(found);
                self.captions_path = Some(candidate);
                return;
            }
        }
    }

    pub fn select(&mut self, index: usize) -> bool {
        let to = match index < self.clips().len() {
            true => index,
            false => usize::MAX,
        };
        let moved = to != self.selected;
        self.selected = to;
        moved
    }

    /// Read the list at a different rate.
    ///
    /// Not an edit — the file does not change and no byte of it moves. What
    /// changes is what the timecodes in it are taken to mean, which is a thing
    /// only a person can know.
    pub fn set_rate(&mut self, fps: u32) -> Result<bool, String> {
        if !(1..=1000).contains(&fps) {
            return Err(format!("{fps} is not a frame rate"));
        }
        if fps == self.fps {
            return Ok(false);
        }
        self.fps = fps;
        // Re-parsed rather than mutated, because the rate is what turns the
        // text into events and the events are derived from the text each time.
        if let Some(timeline) = self.timeline.as_ref() {
            self.timeline = Timeline::parse(timeline.to_text(), fps).ok();
        }
        Ok(true)
    }

    /// Change one field of one event, and write the list.
    pub fn set_clip_field(
        &mut self,
        index: usize,
        field: &str,
        value: &str,
    ) -> Result<bool, String> {
        let Some(timeline) = self.timeline.as_mut() else {
            return Err("no timeline is open".into());
        };
        if !timeline.set_field(index, field, value)? {
            return Ok(false);
        }
        let Some(path) = self.path.clone() else {
            return Err("no timeline is open".into());
        };
        timeline.write(&path)?;
        Ok(true)
    }

    /// Change one field of one caption, and write the sidecar.
    pub fn set_caption_field(
        &mut self,
        index: usize,
        field: &str,
        value: &str,
    ) -> Result<bool, String> {
        let Some(captions) = self.captions.as_mut() else {
            return Err("there is no caption file".into());
        };
        if !captions.set_field(index, field, value)? {
            return Ok(false);
        }
        let Some(path) = self.captions_path.clone() else {
            return Err("there is no caption file".into());
        };
        captions.write(&path)?;
        Ok(true)
    }

    /// Re-read both files: a cut changed in Codex, or by the agent, or by
    /// whatever wrote the transcription.
    pub fn reload(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            return false;
        };
        let mut changed = match Timeline::read(&path, self.fps) {
            Ok(fresh) if self.timeline.as_ref() != Some(&fresh) => {
                self.timeline = Some(fresh);
                self.problem = None;
                self.selected = usize::MAX;
                true
            }
            Ok(_) => false,
            Err(why) => {
                let moved = self.problem.as_deref() != Some(why.as_str());
                self.timeline = None;
                self.problem = Some(why);
                self.selected = usize::MAX;
                moved
            }
        };
        // The sidecar is looked for again rather than only re-read: a
        // transcription that did not exist when the cut was opened is the
        // ordinary case, not an unusual one.
        let before = (self.captions.clone(), self.captions_path.clone());
        self.open_captions(&path);
        changed |= (self.captions.clone(), self.captions_path.clone()) != before;
        changed
    }
}

/// Where a source might be, and whether it is there.
fn locate(beside: Option<&Path>, source: &str, name: &str) -> (String, bool) {
    let Some(beside) = beside else {
        return (String::new(), false);
    };
    for guess in [source, name] {
        if guess.is_empty() {
            continue;
        }
        let named = Path::new(guess);
        // An absolute path out of a `* SOURCE FILE:` comment is where the
        // machine that made the cut kept it, which is usually not this one.
        for at in [
            named.to_path_buf(),
            beside.join(guess),
            beside.join("media").join(guess),
        ] {
            if at.is_file() {
                return (at.display().to_string(), true);
            }
        }
    }
    (String::new(), false)
}

/// The caption files worth trying beside a list, best first.
fn sidecars(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(stem) = path.file_stem() {
        out.push(path.with_file_name(format!("{}.srt", stem.to_string_lossy())));
    }
    if let Some(folder) = path.parent() {
        if let Ok(entries) = std::fs::read_dir(folder) {
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "srt"))
                .collect();
            // Sorted, so the same cut opens the same captions twice.
            found.sort();
            out.extend(found);
        }
    }
    out
}

/// Look for a cut under the project.
///
/// Only files that really parse: `.edl` is a common enough extension that
/// finding one proves nothing about what is in it.
pub fn discover(root: &Path) -> Option<PathBuf> {
    const DIRS: &[&str] = &["", "edit", "cut", "video", "timeline", "edl"];
    for dir in DIRS {
        let at = match dir.is_empty() {
            true => root.to_path_buf(),
            false => root.join(dir),
        };
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "edl"))
            .collect();
        // Sorted, so the same project opens the same cut twice.
        found.sort();
        if let Some(first) = found
            .into_iter()
            .find(|path| Timeline::read(path, ASSUMED_FPS).is_ok())
        {
            return Some(first);
        }
    }
    None
}
