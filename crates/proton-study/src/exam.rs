//! Exams: papers like the course's past papers, and the past papers
//! themselves, to be sat against the clock.
//!
//! A practice exam is set by one call that reads the past papers whole
//! first — their length, sections and marks, the kinds of question, the
//! topics they test and how — says so as the paper's blueprint, and sets a
//! new paper like them: the same sections and marks, every question new.
//! Each question is held against the past papers' words, as Research holds
//! hypotheses against each other ([`proton_research::proximity`]): one too
//! like a past paper's is sent back to be set afresh, once, and is marked as
//! like it if it still is.
//!
//! A past paper is the real one, transcribed: its questions in its own
//! words, each citing its page, marked by a scheme drawn from its solutions
//! when the notebook has them.
//!
//! Both are quizzes ([`crate::quiz`]) with sections, marks and minutes. A
//! sitting is an attempt that ends when its time does, gives no hints, keeps
//! its answers as drafts as they are written, and marks nothing until it is
//! handed in.

use proton_library::Hit;
use serde_json::{json, Value};

use crate::model::{Role, Source};
use crate::quiz::{self, QuestionKind, Quiz, StudyPaperSection, StudyQuestion};
use crate::store::Book;
use crate::snapshot;
use crate::studio::{Level, Options};

/// How alike a question and a past paper's paragraph are, at most, before
/// the question is a past paper's reworded.
const NEAR: f64 = 0.6;

/// The notebook's past papers whose words it has.
pub fn past_papers(book: &Book) -> Vec<&Source> {
    book.sources.sources.iter().filter(|s| s.role == Role::PastPaper && s.status.has_words()).collect()
}

/// A title's words that tell one paper from another: its year, its name.
fn telling(title: &str) -> Vec<String> {
    let stem = title.rsplit_once('.').map_or(title, |(stem, _)| stem);
    stem.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| !w.is_empty() && !matches!(w.as_str(), "solution" | "solutions" | "sol" | "sols" | "answers" | "answer" | "memo" | "key" | "marking" | "scheme" | "with"))
        .collect()
}

/// The solutions to a past paper, among the notebook's sources: the one
/// whose name shares the most of the paper's.
pub fn solutions_for(book: &Book, paper: &str) -> Option<String> {
    let paper = book.sources.get(paper)?;
    let words = telling(&paper.title);
    book.sources
        .sources
        .iter()
        .filter(|s| s.role == Role::Solution && s.status.has_words())
        .map(|s| (s, telling(&s.title).iter().filter(|w| words.contains(w)).count()))
        .filter(|(s, shared)| *shared > 0 && telling(&s.title).len() <= words.len() + 1)
        .max_by_key(|(_, shared)| *shared)
        .map(|(s, _)| s.id.clone())
}

/// What a practice exam is asked to be.
pub fn prompt(book: &Book, options: &Options, course: &str) -> String {
    let course = match course.trim() {
        "" => "the course".to_string(),
        course => course.to_string(),
    };
    let papers = past_papers(book);
    let like = match papers.is_empty() {
        true => "There are no past papers among the sources: set a paper of two hours in three or four sections, as a university final \
                 examines the course, and say so as its blueprint."
            .to_string(),
        false => format!(
            "The past papers among the sources are {}. Read them whole first, and write as the paper's blueprint what they are like: \
             how long they are, their sections and instructions, how many marks each part carries, the kinds of question, the topics \
             they examine and how often, and their style. Then set a new paper like them — the same length, sections and marks, the \
             same kinds of question, the topics weighed as they weigh them — but every question new: never a past paper's question \
             reworded, nor its numbers changed.",
            papers.iter().map(|s| format!("{} ({})", s.id, s.title)).collect::<Vec<_>>().join(", ")
        ),
    };
    let level = match options.level {
        Level::Medium => String::new(),
        level => format!(" Make it a little {} than they are.", if level == Level::Easy { "easier" } else { "harder" }),
    };
    let focus = match options.focus.trim() {
        "" => String::new(),
        focus => format!(" Weigh it toward: {focus}."),
    };
    format!(
        "Set a practice exam for {course}. {like}{level}{focus} Every question has its marks; a written one, a rubric whose points sum \
         to them and a full model answer; each has an explanation, its topic, the objectives it tests and citations of the notes and \
         slides that teach it — not of the past papers. No hints: it is sat as an exam. Leave unused fields empty."
    )
}

/// What a past paper's transcription is asked to be.
pub fn prompt_past_paper(book: &Book, paper: &str) -> Result<String, String> {
    let source = book.sources.get(paper).ok_or_else(|| format!("{paper}: no such source"))?;
    let marked = match solutions_for(book, paper).and_then(|id| book.sources.get(&id)) {
        Some(solutions) => format!(
            "Its solutions are {} ({}): each question's model answer, explanation and rubric come from them, cited, its rubric's points \
             summing to the question's marks as the paper gives them.",
            solutions.id, solutions.title
        ),
        None => "There are no solutions to it among the sources: write each question's model answer, explanation and rubric from the \
                 other sources, cited, the rubric's points summing to the question's marks."
            .to_string(),
    };
    Ok(format!(
        "Transcribe the past paper {} ({}) into its questions, to be sat again: each in the paper's own words, with its parts, the marks \
         the paper gives it, and the page it is on as its first citation ([[{} p. 2]]), in the paper's sections with their instructions, \
         and the paper's duration in minutes if it says it. Keep each question's kind as the paper asks it: a choice when it gives \
         options, a number when it asks for one, blanks when it has them; otherwise written, short or long. {marked} As the blueprint, \
         say what the paper is: its year, length, sections and marks. No hints. Leave unused fields empty.",
        source.id, source.title, source.id
    ))
}

/// The JSON an exam paper must be.
pub fn schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["title", "minutes", "blueprint", "sections"],
        "properties": {
            "title": { "type": "string" },
            "minutes": { "type": "integer", "description": "How long it is sat for; 0 if the paper does not say" },
            "blueprint": { "type": "string", "description": "What the past papers it is modelled on are like, in Markdown" },
            "sections": { "type": "array", "items": {
                "type": "object", "additionalProperties": false, "required": ["title", "instructions", "questions"],
                "properties": {
                    "title": { "type": "string" },
                    "instructions": { "type": "string" },
                    "questions": { "type": "array", "items": quiz::question_schema(true) }
                }
            }}
        }
    })
}

/// An exam paper, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Paper {
    pub title: String,
    pub minutes: Option<u32>,
    pub blueprint: String,
    pub sections: Vec<StudyPaperSection>,
    pub questions: Vec<StudyQuestion>,
}

/// A paper an answer gave, read: its sections, and its questions each whole
/// and numbered through the paper. `Err` when a third or more are not.
pub fn read_paper(value: &Value) -> Result<Paper, String> {
    let said: Vec<Value> = serde_json::from_value(value["sections"].clone()).map_err(|e| format!("its sections: {e}"))?;
    let mut sections = Vec::new();
    let mut questions: Vec<StudyQuestion> = Vec::new();
    let (mut total, mut wrong) = (0usize, Vec::new());
    for section in &said {
        let mut ids = Vec::new();
        for (n, q) in section["questions"].as_array().into_iter().flatten().enumerate() {
            total += 1;
            match quiz::read_question(q) {
                Ok(mut q) => {
                    q.id = format!("q{:02}", questions.len() + 1);
                    // A question of an exam is worth at least what its kind is.
                    if q.marks.is_none() {
                        q.marks = Some(q.points());
                    }
                    ids.push(q.id.clone());
                    questions.push(q);
                }
                Err(why) => wrong.push(format!("{} question {}: {why}", section["title"].as_str().unwrap_or("a section"), n + 1)),
            }
        }
        if !ids.is_empty() {
            sections.push(StudyPaperSection { title: section["title"].as_str().unwrap_or_default().trim().to_string(), instructions: section["instructions"].as_str().unwrap_or_default().trim().to_string(), questions: ids });
        }
    }
    if questions.is_empty() {
        return Err("the paper has no questions".into());
    }
    if wrong.len() * 3 >= total {
        return Err(format!("only {} of its {total} questions are whole — {}", questions.len(), wrong.join("; ")));
    }
    let minutes = value["minutes"].as_u64().filter(|m| *m > 0 && *m <= 600).map(|m| m as u32);
    Ok(Paper {
        title: value["title"].as_str().unwrap_or_default().trim().to_string(),
        minutes,
        blueprint: value["blueprint"].as_str().unwrap_or_default().trim().to_string(),
        sections,
        questions,
    })
}

/// A past paper's paragraphs, each with the citation of the passage it is
/// in: what a question is held against.
fn paragraphs(hits: &[Hit]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for hit in hits {
        let mut current = String::new();
        let numbered = |line: &str| {
            let t = line.trim_start();
            t.starts_with("Question") || t.starts_with('(') && t.chars().nth(2) == Some(')') || t.split_once(['.', ')']).is_some_and(|(n, _)| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit()))
        };
        for line in hit.passage.text.lines() {
            if (line.trim().is_empty() || numbered(line)) && current.trim().len() >= 40 {
                out.push((hit.cite.clone(), std::mem::take(&mut current)));
            }
            current.push_str(line);
            current.push('\n');
        }
        if current.trim().len() >= 40 {
            out.push((hit.cite.clone(), current));
        }
    }
    out
}

/// The questions too like a past paper's: each with the citation of the
/// paragraph it is like.
pub fn near_copies(book: &Book, questions: &[StudyQuestion]) -> Vec<(String, String)> {
    let papers: Vec<String> = past_papers(book).iter().map(|s| s.id.clone()).collect();
    if papers.is_empty() || questions.is_empty() {
        return Vec::new();
    }
    let hits: Vec<Hit> = snapshot::library(book).all().into_iter().filter(|h| papers.contains(&h.doc)).collect();
    let paragraphs = paragraphs(&hits);
    let documents: Vec<(String, String)> =
        questions.iter().map(|q| (format!("q:{}", q.id), q.prompt.clone())).chain(paragraphs.iter().enumerate().map(|(i, (_, text))| (format!("p:{i}"), text.clone()))).collect();
    let index = proton_research::proximity::Index::new(documents.iter().map(|(id, text)| (id.as_str(), text.as_str())));
    let mut out = Vec::new();
    for q in questions {
        let id = format!("q:{}", q.id);
        let best = (0..paragraphs.len()).map(|i| (i, index.similarity(&id, &format!("p:{i}")))).max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, s)) = best {
            if s >= NEAR {
                out.push((q.id.clone(), paragraphs[i].0.clone()));
            }
        }
    }
    out
}

/* ── out ────────────────────────────────────────────────────────────── */

fn marks(q: &StudyQuestion) -> String {
    let m = q.points();
    format!("[{} {}]", trim(m), if m == 1.0 { "mark" } else { "marks" })
}

fn trim(n: f64) -> String {
    match n.fract() == 0.0 {
        true => format!("{}", n as i64),
        false => format!("{n}"),
    }
}

/// A question's answer, as a marking scheme says it.
fn answer(q: &StudyQuestion) -> String {
    let letter = |i: usize| char::from(b'a' + i as u8);
    match q.kind {
        QuestionKind::Choice | QuestionKind::Choices => q.correct.iter().map(|&i| format!("({}) {}", letter(i), q.options.get(i).cloned().unwrap_or_default())).collect::<Vec<_>>().join("; "),
        QuestionKind::Blanks => q.blanks.iter().map(|b| b.join(" / ")).collect::<Vec<_>>().join("; "),
        QuestionKind::Number => format!("{}{}{}", q.number.map(|n| format!("{n}")).unwrap_or_default(), if q.unit.is_empty() { String::new() } else { format!(" {}", q.unit) }, if q.tolerance > 0.0 { format!(" (± {})", q.tolerance) } else { String::new() }),
        QuestionKind::Short | QuestionKind::Long => q.model_answer.clone(),
    }
}

/// A paper as Markdown: the questions alone, or with their marking scheme.
pub fn markdown(quiz: &Quiz, scheme: bool) -> String {
    let total: f64 = quiz.questions.iter().map(StudyQuestion::points).sum();
    let mut out = format!("# {}{}\n\n", quiz.title, if scheme { " — marking scheme" } else { "" });
    let mut facts = Vec::new();
    if let Some(minutes) = quiz.minutes {
        facts.push(format!("{minutes} minutes"));
    }
    facts.push(format!("{} marks", trim(total)));
    out.push_str(&format!("*{}*\n\n", facts.join(" · ")));
    let sections: Vec<StudyPaperSection> = match quiz.sections.is_empty() {
        true => vec![StudyPaperSection { title: String::new(), instructions: String::new(), questions: quiz.questions.iter().map(|q| q.id.clone()).collect() }],
        false => quiz.sections.clone(),
    };
    let mut n = 0;
    for section in &sections {
        if !section.title.is_empty() {
            out.push_str(&format!("## {}\n\n", section.title));
        }
        if !section.instructions.is_empty() {
            out.push_str(&format!("*{}*\n\n", section.instructions));
        }
        for id in &section.questions {
            let Some(q) = quiz.question(id) else { continue };
            n += 1;
            out.push_str(&format!("**{n}.** {} {}\n\n", q.prompt, marks(q)));
            for (i, option) in q.options.iter().enumerate() {
                out.push_str(&format!("- ({}) {option}\n", char::from(b'a' + i as u8)));
            }
            if !q.options.is_empty() {
                out.push('\n');
            }
            if scheme {
                out.push_str(&format!("> **Answer.** {}\n", answer(q).replace('\n', "\n> ")));
                if !q.rubric.is_empty() {
                    out.push_str(">\n");
                    for c in &q.rubric {
                        out.push_str(&format!("> - {} {}: {}\n", trim(c.points), if c.points == 1.0 { "mark" } else { "marks" }, c.text));
                    }
                }
                if !q.explanation.is_empty() && q.kind != QuestionKind::Long {
                    out.push_str(&format!(">\n> {}\n", q.explanation.replace('\n', "\n> ")));
                }
                out.push('\n');
            }
        }
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Markdown's inline marks as HTML: bold and code, the rest escaped.
fn inline(text: &str) -> String {
    let mut rest = escape(text);
    for (mark, open, close) in [("**", "<strong>", "</strong>"), ("`", "<code>", "</code>")] {
        let mut done = String::new();
        while let Some(start) = rest.find(mark) {
            let Some(len) = rest[start + mark.len()..].find(mark) else { break };
            done.push_str(&rest[..start]);
            done.push_str(open);
            done.push_str(&rest[start + mark.len()..start + mark.len() + len]);
            done.push_str(close);
            rest = rest[start + mark.len() * 2 + len..].to_string();
        }
        done.push_str(&rest);
        rest = done;
    }
    rest
}

/// Markdown as HTML, as much of it as an exam's text uses: paragraphs,
/// lists, quotes, headings, code, bold — its TeX left for KaTeX to draw.
pub fn html_of(markdown: &str) -> String {
    let mut out = String::new();
    let mut list: Option<&str> = None;
    let mut paragraph: Vec<String> = Vec::new();
    let mut code = false;
    let close_paragraph = |out: &mut String, paragraph: &mut Vec<String>| {
        if !paragraph.is_empty() {
            out.push_str(&format!("<p>{}</p>\n", paragraph.join("<br>\n")));
            paragraph.clear();
        }
    };
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            close_paragraph(&mut out, &mut paragraph);
            out.push_str(if code { "</pre>\n" } else { "<pre>" });
            code = !code;
            continue;
        }
        if code {
            out.push_str(&escape(line));
            out.push('\n');
            continue;
        }
        let trimmed = line.trim();
        let item = trimmed.strip_prefix("- ").map(|t| ("ul", t)).or_else(|| trimmed.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).map(|(_, t)| ("ol", t)));
        if list.is_some() && item.map(|(k, _)| k) != list {
            out.push_str(&format!("</{}>\n", list.unwrap_or("ul")));
            list = None;
        }
        if let Some((kind, text)) = item {
            close_paragraph(&mut out, &mut paragraph);
            if list.is_none() {
                out.push_str(&format!("<{kind}>\n"));
                list = Some(kind);
            }
            out.push_str(&format!("<li>{}</li>\n", inline(text)));
        } else if trimmed.is_empty() {
            close_paragraph(&mut out, &mut paragraph);
        } else if let Some(heading) = trimmed.strip_prefix("## ") {
            close_paragraph(&mut out, &mut paragraph);
            out.push_str(&format!("<h2>{}</h2>\n", inline(heading)));
        } else if let Some(heading) = trimmed.strip_prefix("# ") {
            close_paragraph(&mut out, &mut paragraph);
            out.push_str(&format!("<h1>{}</h1>\n", inline(heading)));
        } else if let Some(quoted) = trimmed.strip_prefix('>') {
            close_paragraph(&mut out, &mut paragraph);
            out.push_str(&format!("<blockquote>{}</blockquote>\n", inline(quoted.trim())));
        } else {
            paragraph.push(inline(trimmed));
        }
    }
    close_paragraph(&mut out, &mut paragraph);
    if let Some(kind) = list {
        out.push_str(&format!("</{kind}>\n"));
    }
    if code {
        out.push_str("</pre>\n");
    }
    out
}

/// A paper as a page to print: its Markdown as HTML, its TeX drawn by KaTeX.
pub fn html(quiz: &Quiz, scheme: bool) -> String {
    let body = html_of(&markdown(quiz, scheme));
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n\
         <link rel=\"stylesheet\" href=\"https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css\">\n\
         <script defer src=\"https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js\"></script>\n\
         <script defer src=\"https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js\" \
         onload=\"renderMathInElement(document.body, {{delimiters: [{{left: '$$', right: '$$', display: true}}, {{left: '$', right: '$', display: false}}]}})\"></script>\n\
         <style>body {{ max-width: 46rem; margin: 2.5rem auto; padding: 0 1.25rem; font: 16px/1.6 Georgia, serif; color: #111; }}\n\
         h1 {{ font-size: 1.6rem; margin-bottom: .2rem; }} h2 {{ font-size: 1.2rem; margin-top: 2rem; border-bottom: 1px solid #ccc; }}\n\
         blockquote {{ margin: .4rem 0 .4rem 1rem; padding-left: .8rem; border-left: 3px solid #999; color: #333; }}\n\
         pre {{ background: #f4f4f4; padding: .6rem; overflow-x: auto; }} @media print {{ body {{ margin: 0; }} h2 {{ break-after: avoid; }} }}</style>\n\
         </head><body>\n{body}</body></html>\n",
        title = escape(&quiz.title)
    )
}

/// A paper's blueprint and sections, said in a line for a list.
pub fn summary(quiz: &Quiz) -> String {
    let total: f64 = quiz.questions.iter().map(StudyQuestion::points).sum();
    let mut said = vec![format!("{} questions", quiz.questions.len()), format!("{} marks", trim(total))];
    if let Some(m) = quiz.minutes {
        said.insert(0, format!("{m} minutes"));
    }
    said.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Origin, SourceStatus, Units};
    use crate::store::Folder;
    use proton_library::extract::Body;

    fn book() -> Book {
        let root = std::env::temp_dir().join(format!("proton-study-exam-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        Folder::new(root).create("Search", 1).unwrap()
    }

    fn source(book: &mut Book, title: &str, role: Role, words: &str) -> String {
        let id = book.sources.take_id();
        book.sources.sources.push(Source {
            id: id.clone(),
            title: title.into(),
            kind: "markdown".into(),
            origin: Origin::Pasted,
            role,
            size: words.len() as u64,
            mtime: 1,
            sha256: crate::snapshot::digest_text(words),
            units: Units::Lines,
            count: words.lines().count(),
            thin: Vec::new(),
            status: SourceStatus::Ready,
            added: 1,
            labels: Vec::new(),
        });
        crate::snapshot::keep(book, &id, &Body::Lines(words.to_string())).unwrap();
        id
    }

    fn paper_value() -> Value {
        json!({ "title": "Practice final", "minutes": 120, "blueprint": "Two sections; 20 marks.", "sections": [
            { "title": "Section A", "instructions": "Answer all.", "questions": [
                { "kind": "choice", "prompt": "Which is complete?", "options": ["DFS", "BFS"], "correct": [1], "marks": 2 },
                { "kind": "number", "prompt": "How many nodes?", "number": 7, "marks": 0 }
            ]},
            { "title": "Section B", "instructions": "", "questions": [
                { "kind": "long", "prompt": "Prove UCS optimal.", "rubric": [{ "id": "a", "text": "order", "points": 6 }, { "id": "b", "text": "ε", "points": 4 }], "modelAnswer": "…", "marks": 8 },
                { "kind": "short", "prompt": "", "rubric": [], "modelAnswer": "" }
            ]}
        ]})
    }

    #[test]
    fn a_paper_reads_into_sections_numbered_through() {
        let paper = read_paper(&paper_value()).unwrap();
        assert_eq!((paper.title.as_str(), paper.minutes), ("Practice final", Some(120)));
        assert_eq!(paper.sections.iter().map(|s| s.questions.clone()).collect::<Vec<_>>(), [vec!["q01".to_string(), "q02".to_string()], vec!["q03".to_string()]]);
        assert_eq!(paper.questions.iter().map(StudyQuestion::points).collect::<Vec<_>>(), [2.0, 1.0, 10.0], "a choice of 2 marks; a number of none is 1; a proof as its rubric");
        let mostly_broken = json!({ "sections": [{ "title": "A", "questions": [{ "kind": "choice", "prompt": "?", "options": [] }, { "kind": "number", "prompt": "n?", "number": 2 }] }] });
        assert!(read_paper(&mostly_broken).unwrap_err().contains("only 1 of its 2"));
    }

    #[test]
    fn a_past_paper_finds_its_solutions_and_a_question_like_it_is_caught() {
        let mut book = book();
        let paper = source(&mut book, "Exam_2020.pdf", Role::PastPaper, "# Final 2020\n\n1. Explain why uniform-cost search is optimal when every step cost is at least epsilon.\n\n2. Draw the search tree for breadth-first search on the graph below and give the order nodes are expanded.\n");
        let _other = source(&mut book, "Exam_2021_solutions.pdf", Role::Solution, "Solutions 2021");
        let solutions = source(&mut book, "Exam_2020_solutions.pdf", Role::Solution, "Solutions 2020");
        source(&mut book, "Lecture 3.pdf", Role::Slides, "Slides");
        assert_eq!(solutions_for(&book, &paper), Some(solutions.clone()));
        let asked = prompt_past_paper(&book, &paper).unwrap();
        assert!(asked.contains("Its solutions are S03 (Exam_2020_solutions.pdf)") && asked.contains("[[S01 p. 2]]"), "{asked}");
        let set = prompt(&book, &Options::default(), "COMP3670");
        assert!(set.contains("The past papers among the sources are S01 (Exam_2020.pdf)") && set.contains("every question new"), "{set}");
        let question = |id: &str, prompt: &str| StudyQuestion { id: id.into(), prompt: prompt.into(), ..read_paper(&paper_value()).unwrap().questions[0].clone() };
        let near = near_copies(&book, &[
            question("q01", "Explain why uniform-cost search is optimal when every step cost is at least epsilon."),
            question("q02", "Compare the memory used by iterative deepening with that of A* on a tree of depth d."),
        ]);
        assert_eq!(near.len(), 1, "{near:?}");
        assert_eq!(near[0].0, "q01");
        assert!(near[0].1.starts_with("S01"), "{near:?}");
    }

    #[test]
    fn a_paper_goes_out_as_markdown_and_as_a_page_to_print() {
        let paper = read_paper(&paper_value()).unwrap();
        let quiz = Quiz {
            kind: crate::studio::Make::Exam,
            title: paper.title.clone(),
            made: 1,
            by: String::new(),
            task: String::new(),
            engine: String::new(),
            questions: paper.questions.clone(),
            sections: paper.sections.clone(),
            minutes: paper.minutes,
            blueprint: paper.blueprint.clone(),
            source: None,
        };
        let plain = markdown(&quiz, false);
        assert!(plain.starts_with("# Practice final\n\n*120 minutes · 13 marks*") && plain.contains("## Section A") && plain.contains("**1.** Which is complete? [2 marks]"), "{plain}");
        assert!(plain.contains("- (b) BFS") && !plain.contains("Answer."));
        let scheme = markdown(&quiz, true);
        assert!(scheme.contains("> **Answer.** (b) BFS") && scheme.contains("> - 6 marks: order"), "{scheme}");
        let page = html(&quiz, false);
        assert!(page.starts_with("<!doctype html>") && page.contains("<h2>Section A</h2>") && page.contains("<li>(b) BFS</li>") && page.contains("katex"), "{page}");
        assert_eq!(html_of("A **b** `c` <x>"), "<p>A <strong>b</strong> <code>c</code> &lt;x&gt;</p>\n");
        assert_eq!(summary(&quiz), "120 minutes · 3 questions · 13 marks");
    }
}
