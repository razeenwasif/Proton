//! The tutor: how it is told to answer, and what is kept of what it said.
//!
//! A conversation with the tutor is kept as `chats/C001.json`: each question
//! and the answer it was given, with that answer's citations resolved as
//! they were when it was given. The engine's own session is not kept — a
//! chat picked up again is a new conversation told what was said — so a
//! notebook's chats read the same whichever engine answered them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cite::StudyCitation;
use crate::model::Notebook;
use crate::store::Book;

/// How the tutor answers: as Gemini Notebook offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyStyle")]
#[serde(rename_all = "camelCase")]
pub enum Style {
    /// Answers, cited.
    #[default]
    Default,
    /// A learning guide: asks before it tells, and leads in steps.
    Guide,
    /// A role the student wrote.
    Custom,
}

/// How long its answers run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyLength")]
#[serde(rename_all = "camelCase")]
pub enum Length {
    Shorter,
    #[default]
    Default,
    Longer,
}

/// The tutor as the notebook has it set: which engine and model answer,
/// and how. Kept in `notebook.json`; a chat keeps its own copy.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyTutorSettings")]
#[serde(rename_all = "camelCase")]
pub struct TutorSettings {
    /// `claude`, `codex`, `gemini` or `antigravity`; empty for Claude Code.
    #[serde(default)]
    pub engine: String,
    /// The engine's model; empty for its default.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub style: Style,
    /// The role, for a custom style: at most [`CUSTOM`] characters.
    #[serde(default)]
    pub custom: String,
    #[serde(default)]
    pub length: Length,
    /// The language to answer in; empty for the question's own.
    #[serde(default)]
    pub language: String,
}

/// The longest a custom role may be.
pub const CUSTOM: usize = 10_000;

/// One question and the answer it was given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyTurn {
    pub asked: String,
    pub answer: String,
    pub citations: Vec<StudyCitation>,
    /// Seconds since the epoch.
    #[ts(type = "number")]
    pub at: u64,
    /// Who answered: the engine, and the model as it said it.
    pub engine: String,
    pub model: String,
    /// Why there is no answer, when the turn failed.
    #[serde(default)]
    pub error: Option<String>,
    /// The notes it wrote, kept in the notebook.
    #[serde(default)]
    pub notes: Vec<StudyTurnNote>,
}

/// A note a tutor wrote in an answer, as the answer shows it once kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyTurnNote {
    pub id: String,
    pub title: String,
    /// A note it had made before, written again.
    pub updated: bool,
}

/// A note as a tutor wrote it: a new one, or one it made before, by its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteDraft {
    pub id: Option<String>,
    pub title: String,
    pub body: String,
}

/// A fence that opens a note: four backticks or more (three will do), or
/// tildes, then `note`, then the note's id if it is one made before, then its
/// title. What follows the fence's own characters is its info.
fn opens_note(line: &str) -> Option<(String, Option<String>, String)> {
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    if n < 3 {
        return None;
    }
    let info = t[n..].trim();
    let rest = info.strip_prefix("note").or_else(|| info.strip_prefix("Note"))?;
    if !(rest.is_empty() || rest.starts_with([' ', ':'])) {
        return None;
    }
    let rest = rest.trim_start_matches(':').trim();
    let (id, title) = match rest.split_once(char::is_whitespace) {
        Some((first, more)) if is_note_id(first.trim_end_matches(':')) => (Some(first.trim_end_matches(':').to_string()), more.trim().trim_start_matches([':', '—', '-']).trim()),
        _ if is_note_id(rest.trim_end_matches(':')) => (Some(rest.trim_end_matches(':').to_string()), ""),
        _ => (None, rest),
    };
    Some((c.to_string().repeat(n), id, title.trim_matches('"').to_string()))
}

fn is_note_id(s: &str) -> bool {
    s.strip_prefix('N').is_some_and(|d| d.len() >= 3 && d.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether a line closes a fence: its character, at least as many, alone.
fn closes(line: &str, fence: &str) -> bool {
    let t = line.trim();
    let c = fence.chars().next().unwrap_or('`');
    t.len() >= fence.len() && t.chars().all(|x| x == c)
}

/// The notes an answer wrote, and the answer without them. A note's title is
/// its fence's, or its first heading's. A fence never closed is left as it
/// was: a note half written is not kept.
pub fn notes_in(answer: &str) -> (String, Vec<NoteDraft>) {
    let lines: Vec<&str> = answer.lines().collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut notes = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some((fence, id, title)) = opens_note(lines[i]) {
            if let Some(end) = (i + 1..lines.len()).find(|&j| closes(lines[j], &fence)) {
                let body = lines[i + 1..end].join("\n").trim().to_string();
                let title = match title.trim() {
                    "" => body.lines().map(|l| l.trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("A note").chars().take(80).collect(),
                    t => t.to_string(),
                };
                if !body.is_empty() || id.is_some() {
                    notes.push(NoteDraft { id, title, body });
                }
                i = end + 1;
                continue;
            }
        }
        kept.push(lines[i]);
        i += 1;
    }
    let text = kept.join("\n");
    // Where a note stood, no gap of blank lines is left behind.
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        blank = if line.trim().is_empty() { blank + 1 } else { 0 };
        if blank < 2 {
            out.push_str(line);
            out.push('\n');
        }
    }
    (out.trim().to_string(), notes)
}

/// An answer as it comes in: a note it has written shown by its title, and
/// one it is writing said to be on its way, rather than its Markdown.
pub fn writing(answer: &str) -> String {
    let lines: Vec<&str> = answer.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some((fence, _, title)) = opens_note(lines[i]) {
            let named = if title.is_empty() { "a note".to_string() } else { format!("the note “{title}”") };
            match (i + 1..lines.len()).find(|&j| closes(lines[j], &fence)) {
                Some(end) => {
                    out.push(format!("_Wrote {named}._"));
                    i = end + 1;
                }
                None => {
                    out.push(format!("_Writing {named}…_"));
                    break;
                }
            }
            continue;
        }
        out.push(lines[i].to_string());
        i += 1;
    }
    out.join("\n")
}

/// What a tutor is told of the notes the last answer kept, so it can write
/// one of them again by its id.
pub fn saved(notes: &[StudyTurnNote]) -> String {
    match notes {
        [] => String::new(),
        notes => format!(
            "(Proton kept the notes from your last answer: {}. To change one, write it again with its id.)",
            notes.iter().map(|n| format!("{} “{}”", n.id, n.title)).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// A chat: `chats/C001.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    #[serde(skip)]
    pub id: String,
    pub title: String,
    pub created: u64,
    pub updated: u64,
    pub settings: TutorSettings,
    pub turns: Vec<StudyTurn>,
    /// The learning objective it is a lesson on, if it is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lesson: Option<String>,
}

/// A chat, as a list of them shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyChatSummary {
    pub id: String,
    pub title: String,
    #[ts(type = "number")]
    pub updated: u64,
    pub turns: usize,
}

impl Book {
    /// Every chat, the latest first.
    pub fn chats(&self) -> Vec<StudyChatSummary> {
        let Ok(entries) = std::fs::read_dir(self.dir().join("chats")) else { return Vec::new() };
        let mut out: Vec<StudyChatSummary> = entries
            .flatten()
            .filter_map(|e| {
                let id = e.file_name().to_str()?.strip_suffix(".json")?.to_string();
                let chat = self.chat(&id)?;
                Some(StudyChatSummary { id, title: chat.title, updated: chat.updated, turns: chat.turns.len() })
            })
            .collect();
        out.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| b.id.cmp(&a.id)));
        out
    }

    pub fn chat(&self, id: &str) -> Option<Chat> {
        if !id.strip_prefix('C').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
        let mut chat: Chat = serde_json::from_str(&std::fs::read_to_string(self.dir().join("chats").join(format!("{id}.json"))).ok()?).ok()?;
        chat.id = id.to_string();
        Some(chat)
    }

    /// The id the next chat is given.
    pub fn new_chat_id(&self) -> String {
        let highest = std::fs::read_dir(self.dir().join("chats"))
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json")?.strip_prefix('C')?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("C{:03}", highest + 1)
    }

    pub fn write_chat(&self, chat: &Chat) -> Result<(), String> {
        let text = serde_json::to_string_pretty(chat).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.dir().join("chats").join(format!("{}.json", chat.id)), format!("{text}\n").as_bytes())
    }
}

/// How a tutor reaches the sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grounding {
    /// Through the notebook's tools: it searches and reads.
    Tools,
    /// Passages are put in each message for it: an engine without the tools.
    Passages,
}

/// What a tutor is told before anything else.
pub fn instructions(notebook: &Notebook, settings: &TutorSettings, grounding: Grounding) -> String {
    let course = match notebook.course.trim() {
        "" => String::new(),
        course => format!(" for {course}"),
    };
    let exam = notebook.exam.as_deref().map(|day| format!(" Their exam is on {day}.")).unwrap_or_default();
    let role = match settings.style {
        Style::Default => format!(
            "You are the tutor of a student's notebook \"{}\"{course}: their own notes, lecture slides, labs, readings and past exam papers.{exam} You answer their questions from those sources.",
            notebook.title
        ),
        Style::Guide => format!(
            "You are a learning guide for a student's notebook \"{}\"{course}: their own notes, lecture slides, labs, readings and past exam papers.{exam} \
             Help them understand, not just know. Before you explain, find out what they already think with a question of your own; then lead them \
             there in small steps, a question or a hint at a time, and let them do the reasoning. Check their understanding before moving on, \
             correct a mistake by asking what led to it, and say plainly when they have it right. Give a full answer only when they ask for one \
             or are stuck after trying.",
            notebook.title
        ),
        Style::Custom => {
            let custom: String = settings.custom.chars().take(CUSTOM).collect();
            format!("You work with a student's notebook \"{}\"{course}: their own notes, slides, labs and past papers.{exam} Your role, as the student wrote it:\n\n{custom}", notebook.title)
        }
    };
    let reach = match grounding {
        Grounding::Tools => "- Call notebook_sources first to see what the notebook holds. Then search_sources — several times, in the words the sources \
                              would use — and read_source around what you find, before you answer. What a source too long to read whole covers, section by section, \
                              is in digest_source. A slide whose words are few can be seen with view_pages.\n- Every claim you take from a source is followed by its citation, copied exactly as the tools give it, in \
                              double brackets: [[S07 p. 12]], [[S03 § Search › Uniform cost L40-58]], [[S12 @ 3:12-3:40]]. Cite each passage \
                              you use, and never cite what you did not read.",
        Grounding::Passages => "- Each message comes with passages from the notebook's sources, found for it, each beginning with its citation in \
                                 double brackets. Answer from those passages.\n- Every claim you take from a passage is followed by its citation, \
                                 copied exactly as the passage gives it: [[S07 p. 12]], [[S03 § Search › Uniform cost L40-58]]. Never cite a \
                                 passage you were not given.",
    };
    let length = match settings.length {
        Length::Shorter => "\n- Keep answers short: the point, and its citations.",
        Length::Default => "",
        Length::Longer => "\n- Answer at length: explain fully, with worked examples where the sources have them.",
    };
    let language = match settings.language.trim() {
        "" => String::new(),
        language => format!("\n- Answer in {language}."),
    };
    format!(
        "{role}\n\nHow you work:\n{reach}\n- Answer from the sources alone. If they do not cover something, say so plainly — \"your sources don't cover \
         this\" — and only then, if it helps, add what is generally known, marked as not from the sources.\n- Write mathematics in TeX: $…$ inline, \
         $$…$$ on lines of its own.\n- Write for a student preparing for an exam: clear and exact, no longer than the question needs, in Markdown.\n- \
         Worked solutions in the notebook are for checking an answer, not for giving one away: when the student is practising, hint before you \
         reveal.\n- When the student asks you to make something to keep — a cheatsheet, a summary, a formula sheet, revision notes — write it as a \
         note: in your answer, between a line ````note Its title and a line ````, four backticks each so code inside it keeps its own fences. \
         Proton saves it in the notebook's notes, where the student can read and edit it, so do not paste it again outside the block; say in a \
         sentence what you made. Write the note in Markdown, citing as in any answer. To change a note you made, open the block with its id: \
         ````note N004 Its title.{length}{language}"
    )
}

/// A message for a tutor given passages rather than tools: the passages
/// found for the question, then the question.
pub fn with_passages(question: &str, passages: &str) -> String {
    match passages.trim() {
        "" => format!("(Nothing in the sources was found for this question.)\n\n---\n\n{question}"),
        passages => format!("Passages from the notebook's sources, found for this question — each begins with its citation:\n\n{passages}\n\n---\n\n{question}"),
    }
}

/// The chat so far, for a new conversation picking it up: what was asked
/// and answered, the newest kept when there is too much.
pub fn earlier(turns: &[StudyTurn], limit: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut used = 0;
    for turn in turns.iter().rev() {
        let said = format!("Student: {}\n\nTutor: {}", turn.asked.trim(), turn.answer.trim());
        if used + said.len() > limit && !out.is_empty() {
            break;
        }
        used += said.len();
        out.push(said);
    }
    out.reverse();
    match out.is_empty() {
        true => String::new(),
        false => format!("Our conversation so far, for context:\n\n{}\n\n(The newest question follows.)", out.join("\n\n")),
    }
}

/// A chat's title, from its first question.
pub fn title(question: &str) -> String {
    let line = question.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("A question");
    let mut title: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        if let Some(space) = title.rfind(' ') {
            title.truncate(space);
        }
        title.push('…');
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Folder;

    fn notebook() -> Notebook {
        Notebook { title: "Search".into(), course: "COMP3670".into(), exam: Some("2026-11-12".into()), created: 1, opened: 1, archived: false, tutor: TutorSettings::default() }
    }

    #[test]
    fn the_tutor_is_told_its_course_how_to_reach_the_sources_and_to_cite() {
        let told = instructions(&notebook(), &TutorSettings::default(), Grounding::Tools);
        assert!(told.contains("\"Search\" for COMP3670") && told.contains("2026-11-12"));
        assert!(told.contains("notebook_sources") && told.contains("[[S07 p. 12]]") && told.contains("don't cover"));
        let passages = instructions(&notebook(), &TutorSettings::default(), Grounding::Passages);
        assert!(!passages.contains("notebook_sources") && passages.contains("passages"));
        let guide = instructions(&notebook(), &TutorSettings { style: Style::Guide, length: Length::Shorter, language: "French".into(), ..Default::default() }, Grounding::Tools);
        assert!(guide.contains("learning guide") && guide.contains("short") && guide.contains("Answer in French."));
        let custom = instructions(&notebook(), &TutorSettings { style: Style::Custom, custom: "Quiz me like a strict examiner.".into(), ..Default::default() }, Grounding::Tools);
        assert!(custom.contains("strict examiner") && custom.contains("[[S07 p. 12]]"), "a custom role still cites");
    }

    #[test]
    fn a_chat_is_kept_and_listed_the_latest_first() {
        let root = std::env::temp_dir().join(format!("proton-study-chats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let book = Folder::new(root).create("Search", 1).unwrap();
        assert_eq!(book.new_chat_id(), "C001");
        let turn = StudyTurn { asked: "Why is UCS optimal?".into(), answer: "Because [[S01 p. 1]].".into(), citations: Vec::new(), at: 5, engine: "claude".into(), model: "m".into(), error: None, notes: Vec::new() };
        for (id, updated) in [("C001", 10), ("C002", 20)] {
            book.write_chat(&Chat { id: id.into(), title: title(&turn.asked), created: 1, updated, settings: TutorSettings::default(), turns: vec![turn.clone()], lesson: None }).unwrap();
        }
        assert_eq!(book.chats().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["C002", "C001"]);
        assert_eq!(book.chat("C001").unwrap().turns, [turn]);
        assert_eq!(book.new_chat_id(), "C003");
        assert!(book.chat("../notebook").is_none());
    }

    #[test]
    fn the_chat_so_far_is_told_newest_kept() {
        let turn = |n: usize| StudyTurn { asked: format!("question {n}"), answer: "x".repeat(400), citations: Vec::new(), at: 0, engine: String::new(), model: String::new(), error: None, notes: Vec::new() };
        let told = earlier(&[turn(1), turn(2), turn(3)], 900);
        assert!(told.contains("question 3") && told.contains("question 2") && !told.contains("question 1"), "{told}");
        assert_eq!(earlier(&[], 900), "");
        assert_eq!(title("  \nWhy is uniform-cost search optimal when step costs are positive?"), "Why is uniform-cost search optimal when step costs are positive?");
    }

    #[test]
    fn a_note_in_an_answer_is_taken_out_whole_with_its_code_and_its_title() {
        let answer = "Here is your cheatsheet.\n\n````note Graph search cheatsheet\n## Uniform cost\n\nExpands the cheapest node [[S01 p. 3]].\n\n```python\nheapq.heappush(q, (g, n))\n```\n````\n\nAsk if you want more.";
        let (text, notes) = notes_in(answer);
        assert_eq!(text, "Here is your cheatsheet.\n\nAsk if you want more.");
        assert_eq!(notes.len(), 1);
        assert_eq!((notes[0].id.as_deref(), notes[0].title.as_str()), (None, "Graph search cheatsheet"));
        assert!(notes[0].body.starts_with("## Uniform cost") && notes[0].body.ends_with("heapq.heappush(q, (g, n))\n```"), "{}", notes[0].body);
        // One made before, by its id; a title from its heading; tildes.
        let (_, notes) = notes_in("~~~note N004\n# Formulas\n$a^2$\n~~~\n````Note: N012 — Revised\nx\n````");
        assert_eq!(notes.iter().map(|n| (n.id.as_deref(), n.title.as_str())).collect::<Vec<_>>(), [(Some("N004"), "Formulas"), (Some("N012"), "Revised")]);
        // A code block that is not a note stays, and so does a note never closed.
        let plain = "```python\nprint(1)\n```\n````note Half\nunfinished";
        let (text, notes) = notes_in(plain);
        assert!(notes.is_empty());
        assert_eq!(text, plain);
        assert!(notes_in("```notebook\nx\n```").1.is_empty(), "a word starting with note is not a note");
    }

    #[test]
    fn a_note_on_its_way_is_said_to_be_rather_than_shown() {
        assert_eq!(writing("Making it.\n\n````note Sheet\n## A\nsome"), "Making it.\n\n_Writing the note “Sheet”…_");
        assert_eq!(writing("````note Sheet\nx\n````\nDone."), "_Wrote the note “Sheet”._\nDone.");
        assert_eq!(writing("No note here."), "No note here.");
        let said = saved(&[StudyTurnNote { id: "N004".into(), title: "Sheet".into(), updated: false }]);
        assert!(said.contains("N004 “Sheet”"), "{said}");
        assert_eq!(saved(&[]), "");
        let told = instructions(&Notebook { title: "AI".into(), course: String::new(), exam: None, created: 0, opened: 0, archived: false, tutor: TutorSettings::default() }, &TutorSettings::default(), Grounding::Tools);
        assert!(told.contains("````note Its title") && told.contains("````note N004"), "{told}");
    }
}
