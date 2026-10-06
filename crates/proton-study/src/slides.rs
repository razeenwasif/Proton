//! Slide decks: a lecture's slides made from the sources — a title, points
//! that are phrases rather than paragraphs, two columns to compare, a quote
//! — each with speaker notes saying what to say and citing the sources;
//! revised a slide at a time; written out for PowerPoint.
//!
//! ```text
//! studio/slides/P001.json   the deck, slide by slide
//! ```
//!
//! PowerPoint gets the deck through pandoc, as Markdown: a slide a heading,
//! its points a list, two columns as pandoc's columns, its notes as notes —
//! and its TeX as Office's own mathematics, which pandoc writes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::store::Book;
use crate::studio::{Amount, Make};

/// How a slide is laid out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudySlideLayout")]
#[serde(rename_all = "camelCase")]
pub enum Layout {
    /// The deck's title, or a section's.
    Title,
    #[default]
    Points,
    /// Two lists side by side: for a comparison.
    Two,
    /// A line worth a slide of its own.
    Quote,
}

/// A slide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudySlide {
    /// `s01`.
    pub id: String,
    pub layout: Layout,
    pub title: String,
    /// Its points, each a phrase in Markdown; a title slide's subtitle; a quote.
    #[serde(default)]
    pub points: Vec<String>,
    /// The second column's points.
    #[serde(default)]
    pub right: Vec<String>,
    /// What to say, in Markdown, citing the sources.
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub cites: Vec<String>,
}

/// A deck, as its file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slides {
    pub kind: Make,
    pub title: String,
    pub made: u64,
    pub by: String,
    pub task: String,
    pub slides: Vec<StudySlide>,
}

/// About how many slides.
pub fn count(amount: Amount) -> usize {
    match amount {
        Amount::Fewer => 6,
        Amount::Standard => 10,
        Amount::More => 16,
    }
}

/// What a deck is asked to be.
pub fn prompt(amount: Amount, focus: &str, language: &str, course: &str) -> String {
    let course = match course.trim() {
        "" => "the course".to_string(),
        course => course.to_string(),
    };
    let focus = match focus.trim() {
        "" => String::new(),
        focus => format!(" Centre it on: {focus}."),
    };
    let language = match language.trim() {
        "" => String::new(),
        language => format!(" Write it in {language}."),
    };
    format!(
        "Make a slide deck of these sources, for a lecture to students of {course}: about {} slides. The first is a `title` slide, its \
         subtitle its one point. Every other slide has a short title and three to five points, each a phrase rather than a sentence, \
         mathematics in TeX; use `two` to compare — its second list in `right` — and `quote` for a line worth a slide of its own. Every \
         slide has speaker notes saying what to say about it, a paragraph or two, citing the sources; and the citations of what it \
         shows. Leave unused fields empty.{focus}{language}",
        count(amount)
    )
}

/// What a slide's revision is asked to be.
pub fn revise(slide: &StudySlide, ask: &str) -> String {
    format!(
        "Here is a slide of a deck made from these sources, as JSON:\n\n{}\n\nRewrite it as asked: {}. Keep to the sources, citing them; \
         answer with the one slide alone, as the schema says.",
        serde_json::to_string_pretty(slide).unwrap_or_default(),
        ask.trim()
    )
}

fn slide_schema() -> Value {
    let strings = json!({ "type": "array", "items": { "type": "string" } });
    json!({
        "type": "object", "additionalProperties": false, "required": ["layout", "title", "points", "right", "notes", "cites"],
        "properties": {
            "layout": { "type": "string", "enum": ["title", "points", "two", "quote"] },
            "title": { "type": "string" },
            "points": strings,
            "right": strings,
            "notes": { "type": "string" },
            "cites": { "type": "array", "items": { "type": "string" }, "description": "Citations, as written between the brackets: S07 p. 12" }
        }
    })
}

/// The JSON a deck must be — or, revising, one slide.
pub fn schema(one: bool) -> Value {
    match one {
        true => slide_schema(),
        false => json!({
            "type": "object", "additionalProperties": false, "required": ["title", "slides"],
            "properties": { "title": { "type": "string" }, "slides": { "type": "array", "maxItems": 40, "items": slide_schema() } }
        }),
    }
}

/// A slide read: whole, or `None`.
fn slide(value: &Value) -> Option<StudySlide> {
    let mut s: StudySlide = serde_json::from_value(json!({
        "id": "",
        "layout": value["layout"],
        "title": value["title"],
        "points": value["points"],
        "right": value["right"],
        "notes": value["notes"],
        "cites": value["cites"],
    }))
    .ok()?;
    s.title = s.title.trim().to_string();
    s.points.retain(|p| !p.trim().is_empty());
    s.right.retain(|p| !p.trim().is_empty());
    let whole = !s.title.is_empty()
        && match s.layout {
            Layout::Title | Layout::Quote => true,
            Layout::Points => !s.points.is_empty(),
            Layout::Two => !s.points.is_empty() && !s.right.is_empty(),
        };
    whole.then_some(s)
}

/// A deck an answer gave, read: its title and its whole slides, numbered.
pub fn read(value: &Value) -> Result<(String, Vec<StudySlide>), String> {
    let said = value["slides"].as_array().ok_or("it has no slides")?;
    let mut slides: Vec<StudySlide> = said.iter().filter_map(slide).collect();
    if slides.len() < 2 || slides.len() * 2 < said.len() {
        return Err(format!("only {} of its {} slides are whole: each has a title, and a points slide its points", slides.len(), said.len()));
    }
    for (i, s) in slides.iter_mut().enumerate() {
        s.id = format!("s{:02}", i + 1);
    }
    Ok((value["title"].as_str().unwrap_or_default().trim().to_string(), slides))
}

/// A revised slide an answer gave, read, keeping the id it had.
pub fn read_one(value: &Value, id: &str) -> Result<StudySlide, String> {
    let mut s = slide(value).ok_or("the slide is not whole: it has a title, and a points slide its points")?;
    s.id = id.to_string();
    Ok(s)
}

/// A citation's brackets, taken out of what a slide shows.
fn shown(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(end) = rest[start..].find("]]") else { break };
        out.push_str(rest[..start].trim_end());
        rest = &rest[start + end + 2..];
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// A deck as Markdown for pandoc's PowerPoint writer: a slide a heading,
/// its notes as notes, two columns as columns.
pub fn markdown(slides: &Slides, titles: &std::collections::HashMap<String, String>) -> String {
    let yaml = |s: &str| s.replace('"', "\\\"");
    let mut out = format!("---\ntitle: \"{}\"\n---\n\n", yaml(&slides.title));
    let list = |points: &[String]| points.iter().map(|p| format!("- {}\n", shown(p))).collect::<String>();
    for (i, s) in slides.slides.iter().enumerate() {
        // The deck's own title slide is pandoc's, from the title above.
        if i == 0 && s.layout == Layout::Title {
            if let Some(subtitle) = s.points.first() {
                out = format!("---\ntitle: \"{}\"\nsubtitle: \"{}\"\n---\n\n", yaml(&slides.title), yaml(&shown(subtitle)));
            }
            continue;
        }
        out.push_str(&format!("# {}\n\n", shown(&s.title)));
        match s.layout {
            Layout::Title => out.push_str(&s.points.iter().map(|p| format!("{}\n\n", shown(p))).collect::<String>()),
            Layout::Points => out.push_str(&list(&s.points)),
            Layout::Quote => out.push_str(&s.points.iter().map(|p| format!("> {}\n", shown(p))).collect::<String>()),
            Layout::Two => {
                out.push_str("::::: {.columns}\n::: {.column width=\"50%\"}\n");
                out.push_str(&list(&s.points));
                out.push_str(":::\n::: {.column width=\"50%\"}\n");
                out.push_str(&list(&s.right));
                out.push_str(":::\n:::::\n");
            }
        }
        let sources: Vec<String> = s
            .cites
            .iter()
            .map(|c| {
                let (id, place) = c.split_once(' ').unwrap_or((c, ""));
                match titles.get(id) {
                    Some(t) if place.is_empty() => t.clone(),
                    Some(t) => format!("{t}, {place}"),
                    None => c.clone(),
                }
            })
            .collect();
        let notes = shown(&s.notes);
        if !notes.is_empty() || !sources.is_empty() {
            out.push_str("\n::: notes\n");
            out.push_str(&notes);
            if !sources.is_empty() {
                out.push_str(&format!("\n\nFrom: {}", sources.join("; ")));
            }
            out.push_str("\n:::\n");
        }
        out.push('\n');
    }
    out
}

impl Book {
    fn slides_dir(&self) -> PathBuf {
        self.dir().join("studio/slides")
    }

    pub fn new_slides_id(&self) -> String {
        let highest = std::fs::read_dir(self.slides_dir())
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| e.file_name().to_str()?.strip_prefix('P')?.strip_suffix(".json")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("P{:03}", highest + 1)
    }

    pub fn slides(&self, id: &str) -> Option<Slides> {
        if !id.starts_with('P') || id.contains(['/', '\\', '.']) {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(self.slides_dir().join(format!("{id}.json"))).ok()?).ok()
    }

    pub fn save_slides(&self, id: &str, slides: &Slides) -> Result<(), String> {
        let text = serde_json::to_string_pretty(slides).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.slides_dir().join(format!("{id}.json")), format!("{text}\n").as_bytes())
    }

    /// A slide of a deck put in place of what it was.
    pub fn replace_slide(&self, id: &str, slide: StudySlide) -> Result<(), String> {
        let mut deck = self.slides(id).ok_or_else(|| format!("{id}: no such deck of slides"))?;
        let at = deck.slides.iter().position(|s| s.id == slide.id).ok_or_else(|| format!("{}: no such slide", slide.id))?;
        deck.slides[at] = slide;
        self.save_slides(id, &deck)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn deck() -> Value {
        json!({ "title": "Search", "slides": [
            { "layout": "title", "title": "Search", "points": ["How agents find their way"], "right": [], "notes": "", "cites": [] },
            { "layout": "points", "title": "Breadth-first search", "points": ["Layer by layer", "Complete when $b$ is finite [[S01 p. 2]]"], "right": [], "notes": "Say why [[S01 p. 2]].", "cites": ["S01 p. 2"] },
            { "layout": "two", "title": "BFS or DFS", "points": ["Complete"], "right": ["Not complete"], "notes": "", "cites": [] },
            { "layout": "points", "title": "", "points": ["no title"], "right": [], "notes": "", "cites": [] },
            { "layout": "quote", "title": "A rule", "points": ["Never overestimate."], "right": [], "notes": "", "cites": [] }
        ]})
    }

    #[test]
    fn a_deck_reads_its_whole_slides_numbered() {
        let (title, slides) = read(&deck()).unwrap();
        assert_eq!(title, "Search");
        assert_eq!(slides.iter().map(|s| (s.id.as_str(), s.layout)).collect::<Vec<_>>(), [("s01", Layout::Title), ("s02", Layout::Points), ("s03", Layout::Two), ("s04", Layout::Quote)]);
        let revised = read_one(&json!({ "layout": "points", "title": "BFS, again", "points": ["Shallowest first"], "right": [], "notes": "", "cites": [] }), "s02").unwrap();
        assert_eq!((revised.id.as_str(), revised.title.as_str()), ("s02", "BFS, again"));
        assert!(read_one(&json!({ "layout": "two", "title": "x", "points": ["a"], "right": [] }), "s03").is_err(), "two columns need both");
        assert!(revise(&revised, "make it shorter").contains("Rewrite it as asked: make it shorter"));
        assert!(prompt(Amount::More, "", "", "COMP3670").contains("about 16 slides"));
    }

    #[test]
    fn a_deck_goes_to_pandoc_as_slides_with_notes_and_columns() {
        let (title, slides) = read(&deck()).unwrap();
        let kept = Slides { kind: Make::Slides, title, made: 1, by: String::new(), task: String::new(), slides };
        let md = markdown(&kept, &HashMap::from([("S01".to_string(), "Lecture 2".to_string())]));
        assert!(md.starts_with("---\ntitle: \"Search\"\nsubtitle: \"How agents find their way\"\n---\n\n# Breadth-first search"), "{md}");
        assert!(md.contains("- Complete when $b$ is finite\n"), "a citation is not shown on a slide: {md}");
        assert!(md.contains("::: notes\nSay why.\n\nFrom: Lecture 2, p. 2\n:::"), "{md}");
        assert!(md.contains("::::: {.columns}\n::: {.column width=\"50%\"}\n- Complete\n:::\n::: {.column width=\"50%\"}\n- Not complete\n:::\n:::::"), "{md}");
        assert!(md.contains("# A rule\n\n> Never overestimate."));
    }

    #[test]
    fn pandoc_makes_a_powerpoint_of_it_when_it_is_here() {
        let Ok(out) = std::process::Command::new("pandoc").arg("--version").output() else { return };
        if !out.status.success() {
            return;
        }
        let (title, slides) = read(&deck()).unwrap();
        let kept = Slides { kind: Make::Slides, title, made: 1, by: String::new(), task: String::new(), slides };
        let dir = std::env::temp_dir().join(format!("proton-study-slides-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (md, pptx) = (dir.join("deck.md"), dir.join("deck.pptx"));
        std::fs::write(&md, markdown(&kept, &HashMap::new())).unwrap();
        let status = std::process::Command::new("pandoc").args(["-f", "markdown", "-t", "pptx", "-o"]).arg(&pptx).arg(&md).status().unwrap();
        assert!(status.success());
        let bytes = std::fs::read(&pptx).unwrap();
        assert!(bytes.starts_with(b"PK"), "a PowerPoint is a zip");
    }
}
