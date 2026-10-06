//! Audio overviews: two hosts talking a notebook's sources through — or one
//! lecturing — written as a script by one call, and spoken by Fish Audio.
//!
//! ```text
//! studio/audio/A001.json     the script, its chapters, its voices and timings
//! .notebook/audio/A001.mp3   the sound: kept out of Git, as it is the sources read aloud
//! ```
//!
//! A script is segments, each a turn of one speaker's: what is said, with
//! mathematics said in words ("x squared over two"), and the same for the
//! transcript with its mathematics in TeX, and the citations of what it
//! says. It is spoken a piece at a time — no more than two thousand
//! characters or eight turns, each turn tagged `<|speaker:0|>` — and the
//! pieces joined; each segment's place in the sound is its share of its
//! piece's length, so the transcript follows the voice.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::store::Book;
use crate::studio::{Amount, Make};

/// What an audio overview is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyAudioFormat")]
#[serde(rename_all = "camelCase")]
pub enum AudioFormat {
    /// Two hosts unpacking the sources.
    #[default]
    DeepDive,
    /// The essentials, in a few minutes.
    Brief,
    /// Two experts weighing what the sources argue.
    Critique,
    /// Two hosts on opposite sides of what the sources leave open.
    Debate,
    /// One teacher, from the ground up.
    Lecture,
}

impl AudioFormat {
    pub fn title(self) -> &'static str {
        match self {
            AudioFormat::DeepDive => "Deep dive",
            AudioFormat::Brief => "Brief",
            AudioFormat::Critique => "Critique",
            AudioFormat::Debate => "Debate",
            AudioFormat::Lecture => "Lecture",
        }
    }

    /// How many voices speak it.
    pub fn speakers(self) -> usize {
        match self {
            AudioFormat::Lecture => 1,
            _ => 2,
        }
    }

    /// About how long, in minutes.
    pub fn minutes(self, amount: Amount) -> u32 {
        let (fewer, standard, more) = match self {
            AudioFormat::Brief => (2, 3, 5),
            AudioFormat::Lecture => (8, 15, 25),
            _ => (6, 12, 20),
        };
        match amount {
            Amount::Fewer => fewer,
            Amount::Standard => standard,
            Amount::More => more,
        }
    }

    fn says(self) -> &'static str {
        match self {
            AudioFormat::DeepDive => "a deep dive: two hosts in a lively conversation that unpacks the sources — what matters and why, how the ideas \
                                      connect, examples worked through — one asking what a student would ask, the other explaining, and both \
                                      checking they have it",
            AudioFormat::Brief => "a brief: two hosts giving the essentials of the sources and nothing else, plainly",
            AudioFormat::Critique => "a critique: two experts weighing the sources — what they argue well, where they are thin, unclear or \
                                      wrong, and what a student should be wary of",
            AudioFormat::Debate => "a debate: two hosts taking opposite sides on the questions the sources leave open, each arguing from the \
                                    sources, conceding what the other gets right",
            AudioFormat::Lecture => "a lecture: one teacher explaining the sources from the ground up, as to a class, with worked examples and \
                                     a summary at the end",
        }
    }
}

/// A turn of one speaker's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudySegment {
    pub speaker: u8,
    /// What is said: mathematics in words.
    pub text: String,
    /// The same for the transcript: mathematics in TeX.
    pub display: String,
    pub cites: Vec<String>,
}

/// Where a chapter begins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyChapter {
    pub title: String,
    /// The segment it begins at.
    pub segment: usize,
}

/// An audio overview, as its file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Audio {
    pub kind: Make,
    pub title: String,
    pub made: u64,
    pub by: String,
    pub task: String,
    pub format: AudioFormat,
    #[serde(default)]
    pub language: String,
    pub segments: Vec<StudySegment>,
    #[serde(default)]
    pub chapters: Vec<StudyChapter>,
    /// The voices it was spoken in, by Fish's reference id.
    #[serde(default)]
    pub voices: Vec<String>,
    /// Each segment's start and end in the sound, in seconds.
    #[serde(default)]
    pub timings: Vec<[f64; 2]>,
    #[serde(default)]
    pub duration: f64,
    /// `waiting` to be spoken, `voicing`, `ready`, or `failed`.
    pub state: String,
    #[serde(default)]
    pub why: Option<String>,
    /// How many of its pieces are spoken, of how many.
    #[serde(default)]
    pub done: usize,
    #[serde(default)]
    pub of: usize,
}

/// What a script is asked to be.
pub fn prompt(format: AudioFormat, amount: Amount, language: &str, focus: &str, course: &str) -> String {
    let course = match course.trim() {
        "" => "the course".to_string(),
        course => course.to_string(),
    };
    let minutes = format.minutes(amount);
    let words = minutes * 150;
    let speakers = match format.speakers() {
        1 => "one speaker, speaker 0".to_string(),
        _ => "two speakers, 0 and 1, who never name each other".to_string(),
    };
    let language = match language.trim() {
        "" => String::new(),
        language => format!(" It is spoken in {language}."),
    };
    let focus = match focus.trim() {
        "" => String::new(),
        focus => format!(" Centre it on: {focus}."),
    };
    format!(
        "Write the script of an audio overview of these sources for a student of {course}: {}. It runs about {minutes} minutes — about \
         {words} words — for {speakers}.{language}{focus} Every turn is a segment: `text` is what is said aloud, written to be heard — \
         mathematics in words (\"x squared over two\", \"the sum from i equals one to n\"), no symbols, no citations, no lists — and \
         `display` is the same for the transcript, its mathematics in TeX; `cites` are the citations of what the turn says. Begin with what \
         it will cover, and end with what to take away. Group it in chapters, each beginning at a segment.",
        format.says()
    )
}

/// The JSON a script must be.
pub fn schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["title", "chapters", "segments"],
        "properties": {
            "title": { "type": "string" },
            "chapters": { "type": "array", "items": {
                "type": "object", "additionalProperties": false, "required": ["title", "segment"],
                "properties": { "title": { "type": "string" }, "segment": { "type": "integer", "description": "The index of the segment it begins at, from 0" } }
            }},
            "segments": { "type": "array", "maxItems": 600, "items": {
                "type": "object", "additionalProperties": false, "required": ["speaker", "text", "display", "cites"],
                "properties": {
                    "speaker": { "type": "integer", "description": "0, or 1 for the second voice" },
                    "text": { "type": "string", "description": "What is said aloud: mathematics in words" },
                    "display": { "type": "string", "description": "The same, for the transcript: mathematics in TeX" },
                    "cites": { "type": "array", "items": { "type": "string" }, "description": "Citations, as written between the brackets: S07 p. 12" }
                }
            }}
        }
    })
}

/// What is said, as a voice can say it: citations, TeX's dollars and
/// Markdown's marks taken out, and no speaker's tag a turn could fake.
fn spoken(text: &str) -> String {
    let mut out = text.to_string();
    for (open, close) in [("[[", "]]"), ("<|", "|>")] {
        let mut kept = String::new();
        let mut rest = out.as_str();
        while let Some(start) = rest.find(open) {
            let Some(end) = rest[start..].find(close) else { break };
            kept.push_str(&rest[..start]);
            rest = &rest[start + end + close.len()..];
        }
        kept.push_str(rest);
        out = kept;
    }
    let out = out.replace(['$', '*', '#', '`', '_'], "");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A script an answer gave, read: its title, its turns — each said by a
/// speaker there is — and its chapters at segments there are.
pub fn read(value: &Value, format: AudioFormat) -> Result<(String, Vec<StudySegment>, Vec<StudyChapter>), String> {
    #[derive(Deserialize)]
    struct Said {
        speaker: i64,
        text: String,
        #[serde(default)]
        display: String,
        #[serde(default)]
        cites: Vec<String>,
    }
    let said: Vec<Said> = serde_json::from_value(value["segments"].clone()).map_err(|e| format!("its segments: {e}"))?;
    let speakers = format.speakers() as i64;
    let segments: Vec<StudySegment> = said
        .into_iter()
        .filter_map(|s| {
            let text = spoken(&s.text);
            (!text.is_empty()).then(|| StudySegment {
                speaker: s.speaker.clamp(0, speakers - 1) as u8,
                display: match s.display.trim() {
                    "" => s.text.trim().to_string(),
                    display => display.to_string(),
                },
                text,
                cites: s.cites,
            })
        })
        .collect();
    if segments.len() < 3 {
        return Err(format!("a script has turns, each with what is said: this has {}", segments.len()));
    }
    if format.speakers() == 2 && segments.iter().all(|s| s.speaker == segments[0].speaker) {
        return Err("two hosts speak in turn: every segment here is one speaker's".into());
    }
    let mut chapters: Vec<StudyChapter> = serde_json::from_value::<Vec<StudyChapter>>(value["chapters"].clone()).unwrap_or_default().into_iter().filter(|c| c.segment < segments.len() && !c.title.trim().is_empty()).collect();
    chapters.sort_by_key(|c| c.segment);
    chapters.dedup_by_key(|c| c.segment);
    Ok((value["title"].as_str().unwrap_or_default().trim().to_string(), segments, chapters))
}

/// A piece spoken at once: its text, each turn tagged with its speaker, and
/// the segments `[from, to)` it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub text: String,
    pub from: usize,
    pub to: usize,
}

/// The most of a piece: characters, and turns.
pub const CHUNK_CHARS: usize = 2000;
pub const CHUNK_TURNS: usize = 8;

/// A script in pieces to be spoken: as many turns as fit, a turn longer than
/// a piece on its own.
pub fn chunks(segments: &[StudySegment], speakers: usize) -> Vec<Chunk> {
    let tagged = |s: &StudySegment| match speakers {
        1 => s.text.clone(),
        _ => format!("<|speaker:{}|>{}", s.speaker, s.text),
    };
    let mut out: Vec<Chunk> = Vec::new();
    let mut current = Chunk { text: String::new(), from: 0, to: 0 };
    for (i, segment) in segments.iter().enumerate() {
        let turn = tagged(segment);
        let full = current.to > current.from && (current.text.len() + turn.len() + 1 > CHUNK_CHARS || current.to - current.from >= CHUNK_TURNS);
        if full {
            out.push(std::mem::replace(&mut current, Chunk { text: String::new(), from: i, to: i }));
        }
        if !current.text.is_empty() && speakers == 1 {
            current.text.push(' ');
        }
        current.text.push_str(&turn);
        current.to = i + 1;
    }
    if current.to > current.from {
        out.push(current);
    }
    out
}

/// Each segment's place in the sound: its piece's time, shared in
/// proportion to how long each of its turns is.
pub fn timings(segments: &[StudySegment], chunks: &[Chunk], durations: &[f64]) -> Vec<[f64; 2]> {
    let mut out = vec![[0.0, 0.0]; segments.len()];
    let mut start = 0.0;
    for (chunk, duration) in chunks.iter().zip(durations) {
        let lengths: Vec<f64> = (chunk.from..chunk.to).map(|i| segments[i].text.chars().count().max(1) as f64).collect();
        let total: f64 = lengths.iter().sum();
        let mut at = start;
        for (k, i) in (chunk.from..chunk.to).enumerate() {
            let span = duration * lengths[k] / total;
            out[i] = [at, at + span];
            at += span;
        }
        start += duration;
    }
    out
}

impl Book {
    fn audio_dir(&self) -> PathBuf {
        self.dir().join("studio/audio")
    }

    /// Where an overview's sound is: out of Git, in `.notebook/audio/`.
    pub fn audio_file(&self, id: &str) -> PathBuf {
        self.dir().join(".notebook/audio").join(format!("{id}.mp3"))
    }

    /// Where its pieces are, as they are spoken.
    pub fn audio_parts(&self, id: &str) -> PathBuf {
        self.dir().join(".notebook/audio").join(id)
    }

    pub fn new_audio_id(&self) -> String {
        let highest = std::fs::read_dir(self.audio_dir())
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| e.file_name().to_str()?.strip_prefix('A')?.strip_suffix(".json")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("A{:03}", highest + 1)
    }

    pub fn audio(&self, id: &str) -> Option<Audio> {
        if !id.starts_with('A') || id.contains(['/', '\\', '.']) {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(self.audio_dir().join(format!("{id}.json"))).ok()?).ok()
    }

    pub fn save_audio(&self, id: &str, audio: &Audio) -> Result<(), String> {
        let text = serde_json::to_string_pretty(audio).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.audio_dir().join(format!("{id}.json")), format!("{text}\n").as_bytes())
    }

    pub fn audio_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.audio_dir())
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| Some(e.file_name().to_str()?.strip_suffix(".json")?.to_string()))
            .filter(|id| id.starts_with('A'))
            .collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(speaker: u8, text: &str) -> StudySegment {
        StudySegment { speaker, text: text.into(), display: text.into(), cites: vec![] }
    }

    #[test]
    fn a_script_reads_as_turns_said_aloud() {
        let value = json!({ "title": "Search, talked through", "chapters": [{ "title": "Uninformed", "segment": 0 }, { "title": "Past the end", "segment": 99 }, { "title": "Informed", "segment": 2 }],
            "segments": [
                { "speaker": 0, "text": "Today: search. [[S01 p. 2]]", "display": "Today: search.", "cites": ["S01 p. 2"] },
                { "speaker": 1, "text": "Breadth-first search expands the *shallowest* node $first$.", "display": "BFS expands the shallowest node first.", "cites": [] },
                { "speaker": 7, "text": "<|speaker:0|>And A star adds h of n.", "display": "", "cites": [] },
                { "speaker": 0, "text": "   ", "display": "", "cites": [] }
            ]});
        let (title, segments, chapters) = read(&value, AudioFormat::DeepDive).unwrap();
        assert_eq!(title, "Search, talked through");
        assert_eq!(segments.iter().map(|s| (s.speaker, s.text.as_str())).collect::<Vec<_>>(), [
            (0, "Today: search."),
            (1, "Breadth-first search expands the shallowest node first."),
            (1, "And A star adds h of n."),
        ], "citations, marks and a faked tag are not said; an unknown speaker is the second; an empty turn is gone");
        assert_eq!(segments[2].display, "<|speaker:0|>And A star adds h of n.", "with no display, its words as written");
        assert_eq!(chapters.iter().map(|c| c.segment).collect::<Vec<_>>(), [0, 2]);
        let one_voice = json!({ "segments": [{ "speaker": 0, "text": "a" }, { "speaker": 0, "text": "b" }, { "speaker": 0, "text": "c" }] });
        assert!(read(&one_voice, AudioFormat::DeepDive).unwrap_err().contains("two hosts"));
        assert!(read(&one_voice, AudioFormat::Lecture).is_ok(), "a lecture is one voice");
    }

    #[test]
    fn a_script_is_spoken_in_pieces_and_its_turns_timed_by_their_length() {
        let segments: Vec<StudySegment> = (0..20).map(|i| segment((i % 2) as u8, &"word ".repeat(if i == 5 { 500 } else { 10 }))).collect();
        let pieces = chunks(&segments, 2);
        assert!(pieces.iter().all(|c| c.to - c.from <= CHUNK_TURNS), "eight turns at most");
        assert!(pieces.iter().all(|c| c.text.len() <= CHUNK_CHARS || c.to - c.from == 1), "two thousand characters at most, unless one turn is longer");
        assert_eq!(pieces.first().map(|c| c.from), Some(0));
        assert_eq!(pieces.last().map(|c| c.to), Some(20));
        assert!(pieces.windows(2).all(|w| w[0].to == w[1].from), "every turn in one piece");
        assert!(pieces[0].text.starts_with("<|speaker:0|>word") && pieces[0].text.contains("<|speaker:1|>"));
        let lecture = chunks(&[segment(0, "One."), segment(0, "Two.")], 1);
        assert_eq!(lecture[0].text, "One. Two.", "one voice is not tagged");
        let short = [segment(0, "aa"), segment(1, "aaaaaa"), segment(0, "aa")];
        let pieces = chunks(&short, 2);
        let times = timings(&short, &pieces, &[10.0]);
        assert_eq!(times, [[0.0, 2.0], [2.0, 8.0], [8.0, 10.0]]);
        let two = [Chunk { text: String::new(), from: 0, to: 1 }, Chunk { text: String::new(), from: 1, to: 3 }];
        assert_eq!(timings(&short, &two, &[4.0, 8.0])[1], [4.0, 10.0], "the second piece begins where the first ends");
    }

    #[test]
    fn its_asking_says_its_format_its_length_and_its_words() {
        let asked = prompt(AudioFormat::Debate, Amount::Fewer, "French", "heuristics", "COMP3670");
        assert!(asked.contains("a debate") && asked.contains("about 6 minutes") && asked.contains("900 words") && asked.contains("spoken in French") && asked.contains("Centre it on: heuristics"), "{asked}");
        assert!(prompt(AudioFormat::Lecture, Amount::Standard, "", "", "").contains("one speaker"));
    }
}
