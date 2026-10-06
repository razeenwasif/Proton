//! What was found on the web for a notebook: by Discover, pages, papers and
//! videos on a topic, each with what it covers; by Deep Research, a report
//! written from what was read, citing them by number. Kept until the person
//! adds what they want of it as sources, which are then fetched as any page
//! is.
//!
//! ```text
//! studio/found/W001.json   the report, if there is one, and what was found
//! ```
//!
//! Both are calls that search the web and read what they find, and nothing
//! else: no files, no notebook server.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

/// A page found on the web.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyFound {
    pub title: String,
    pub url: String,
    /// What it covers, and why it helps.
    pub why: String,
    /// `page`, `pdf` or `video`.
    pub kind: String,
}

/// The most a call may find.
const MOST: usize = 30;

/// What a call that searches the web works by.
pub fn rules() -> String {
    "You find and read sources on the web for a student. Search with WebSearch, and open each page you would list with WebFetch to see \
     that it is there and says what you say it does: never list a page you have not opened, and give each address exactly as it is. \
     Prefer what a university would set: lecture notes and course pages, open textbooks and their chapters, papers and their \
     preprints, an encyclopaedia's article for a definition, a well-made lecture on video. Leave out pages behind a sign-in or a \
     paywall, forums, and pages written to sell. Mathematics in TeX: $…$ inline, $$…$$ on lines of its own."
        .to_string()
}

/// What the notebook has already, for a call to find others.
fn already(have: &[String]) -> String {
    match have.is_empty() {
        true => String::new(),
        false => format!("\n\nThe notebook has these already; find others:\n{}", have.iter().map(|h| format!("- {h}")).collect::<Vec<_>>().join("\n")),
    }
}

fn of_course(course: &str) -> String {
    match course.trim() {
        "" => "the course".to_string(),
        course => course.to_string(),
    }
}

/// What Discover asks: sources on a topic, each said what it is.
pub fn prompt_discover(topic: &str, course: &str, have: &[String]) -> String {
    format!(
        "Find up to ten sources on the web for a student of {} on: {}. For each, its title, its address, a sentence on what it covers \
         and why it helps, and its kind — `page`, `pdf` or `video`. The best first.{}\n\nAnswer with JSON alone, as the schema says; \
         the report empty.",
        of_course(course),
        topic.trim(),
        already(have)
    )
}

/// What Deep Research asks: a report from what it read, and what it read.
pub fn prompt_research(topic: &str, course: &str, have: &[String]) -> String {
    format!(
        "Research this on the web for a student of {}: {}. Search widely, open and read the best sources — five at least — and write a \
         report from them in Markdown: a # title, an overview, a section (##) for each part of the topic with its key ideas, \
         definitions, results and methods, a worked example where a source has one, and the mistakes students make. Cite every claim \
         with the number of its source in brackets, [3], numbering the sources in the order you list them, and list every source you \
         cite: its title, its address, what it covers, and its kind — `page`, `pdf` or `video`.{}\n\nAnswer with JSON alone, as the \
         schema says.",
        of_course(course),
        topic.trim(),
        already(have)
    )
}

/// The JSON a call's answer must be.
pub fn schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["title", "report", "sources"],
        "properties": {
            "title": { "type": "string" },
            "report": { "type": "string", "description": "The report, in Markdown, citing sources by number: [3]. Empty when only sources were asked for." },
            "sources": { "type": "array", "maxItems": MOST, "items": {
                "type": "object", "additionalProperties": false, "required": ["title", "url", "why", "kind"],
                "properties": {
                    "title": { "type": "string" },
                    "url": { "type": "string" },
                    "why": { "type": "string" },
                    "kind": { "type": "string", "enum": ["page", "pdf", "video"] }
                }
            }}
        }
    })
}

/// A web address, if it is one.
fn web_address(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (host.contains('.') && !url.contains(char::is_whitespace)).then(|| url.to_string())
}

/// What a call found, read: its title, its report — when one was asked
/// for — and the sources, each a web address once.
pub fn read(value: &Value, report: bool) -> Result<(String, String, Vec<StudyFound>), String> {
    let mut found: Vec<StudyFound> = Vec::new();
    for item in value["sources"].as_array().into_iter().flatten() {
        let Some(url) = web_address(item["url"].as_str().unwrap_or_default()) else { continue };
        if found.iter().any(|f| f.url == url) {
            continue;
        }
        let kind = match item["kind"].as_str().unwrap_or_default() {
            kind @ ("page" | "pdf" | "video") => kind.to_string(),
            _ if url.to_ascii_lowercase().split(['?', '#']).next().is_some_and(|u| u.ends_with(".pdf")) => "pdf".into(),
            _ => "page".into(),
        };
        let title = match item["title"].as_str().unwrap_or_default().trim() {
            "" => url.clone(),
            title => title.to_string(),
        };
        found.push(StudyFound { title, url, why: item["why"].as_str().unwrap_or_default().trim().to_string(), kind });
    }
    found.truncate(MOST);
    if found.is_empty() {
        return Err("it found no sources: each is a title, an https address, why it helps, and its kind".into());
    }
    let text = value["report"].as_str().unwrap_or_default().trim().to_string();
    if report && text.chars().filter(|c| c.is_alphanumeric()).count() < 200 {
        return Err("the report is missing: write it from the sources, citing each by its number".into());
    }
    let title = value["title"].as_str().unwrap_or_default().trim().to_string();
    let title = match (title.is_empty(), report) {
        (false, _) => title,
        (true, true) => text.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or_default().trim().to_string(),
        (true, false) => String::new(),
    };
    Ok((title, if report { text } else { String::new() }, found))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_found_is_read_each_address_once_and_only_addresses() {
        let value = json!({ "title": "", "report": "", "sources": [
            { "title": "Uniform-cost search", "url": "https://en.wikipedia.org/wiki/Uniform-cost_search", "why": "The definition.", "kind": "page" },
            { "title": "Again", "url": "https://en.wikipedia.org/wiki/Uniform-cost_search", "why": "", "kind": "page" },
            { "title": "Notes", "url": "https://example.edu/search.PDF?dl=1", "why": "Lecture notes.", "kind": "document" },
            { "title": "", "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "why": "A lecture.", "kind": "video" },
            { "title": "Local", "url": "file:///etc/passwd", "why": "", "kind": "page" },
            { "title": "Nowhere", "url": "https://localhost/x", "why": "", "kind": "page" }
        ]});
        let (title, report, found) = read(&value, false).unwrap();
        assert_eq!((title.as_str(), report.as_str()), ("", ""));
        let urls: Vec<&str> = found.iter().map(|f| f.url.as_str()).collect();
        assert_eq!(urls, ["https://en.wikipedia.org/wiki/Uniform-cost_search", "https://example.edu/search.PDF?dl=1", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"]);
        assert_eq!(found[1].kind, "pdf", "a kind it does not know, from the address");
        assert_eq!(found[2].title, "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "untitled, named by its address");
        assert!(read(&json!({ "sources": [] }), false).is_err());
    }

    #[test]
    fn deep_research_is_a_report_and_what_it_cites() {
        let report = format!("# Uniform-cost search\n\nIt expands the cheapest node first [1], and is optimal when step costs are positive [2]. {}", "More. ".repeat(60));
        let value = json!({ "title": "", "report": report, "sources": [
            { "title": "A", "url": "https://a.example.edu/ucs", "why": "a", "kind": "page" },
            { "title": "B", "url": "https://b.example.edu/ucs.pdf", "why": "b", "kind": "pdf" }
        ]});
        let (title, text, found) = read(&value, true).unwrap();
        assert_eq!((title.as_str(), found.len()), ("Uniform-cost search", 2), "titled by its heading");
        assert!(text.contains("[2]"));
        assert!(read(&json!({ "title": "x", "report": "Too short.", "sources": [{ "title": "A", "url": "https://a.example.edu", "why": "", "kind": "page" }] }), true)
            .unwrap_err()
            .contains("report is missing"));
        assert!(prompt_research("informed search", "COMP3670", &["https://a.example.edu/ucs".into()]).contains("find others:\n- https://a.example.edu/ucs"));
        assert!(prompt_discover("informed search", "", &[]).contains("the course on: informed search"));
        assert!(rules().contains("never list a page you have not opened"));
    }
}
