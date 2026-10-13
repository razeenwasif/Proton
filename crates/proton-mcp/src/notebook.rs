//! One notebook's sources, served to the tutor that answers from them.
//!
//! `--notebook <folder>` serves a Notebook notebook: only the sources ticked
//! for the tutor to use, read from the words the notebook kept when each was
//! added — never from the files they came from, which may have changed or
//! gone to iCloud since — and cited by source id: `[[S07 p. 12]]`. Which are
//! ticked is read again at every call, so ticking a source in the window
//! changes what the next search can see without a new process; the sources'
//! passages are read again whenever `sources.json` changes.
//!
//! `view_pages` draws a PDF source's pages as pictures, for slides whose
//! words were too few to read: it needs the file itself, found under the
//! roots named with `--root Label=/path`, and refuses one in iCloud. The
//! whole process refuses to download from iCloud, so nothing here can.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use proton_library::extract::Body;
use proton_library::{Library, Scope};
use proton_study::model::{Origin, Role, Source, Units};
use proton_study::{snapshot, Book};
use serde_json::{json, Value};

use crate::rpc;

/// How much text one call may hand back.
const MOST: usize = 12_000;
/// How many pages one call may draw.
const PAGES: usize = 4;

/// The notebook, as served: its sources' passages, rebuilt when they change.
pub struct Served {
    dir: PathBuf,
    roots: Vec<(String, PathBuf)>,
    book: Option<Book>,
    library: std::sync::Arc<Library>,
    /// `sources.json` as it was when the passages were last built.
    stamp: Option<SystemTime>,
}

impl Served {
    pub fn open(dir: PathBuf, roots: Vec<(String, PathBuf)>) -> Served {
        let mut served = Served { dir, roots, book: None, library: Default::default(), stamp: None };
        served.fresh();
        served
    }

    /// Read the notebook again if its sources changed.
    fn fresh(&mut self) {
        let stamp = std::fs::metadata(self.dir.join("sources.json")).and_then(|m| m.modified()).ok();
        if self.book.is_some() && stamp == self.stamp {
            return;
        }
        self.stamp = stamp;
        self.book = Book::read(&self.dir).ok();
        self.library = self.book.as_ref().map(snapshot::library).unwrap_or_default();
    }

    /// The sources ticked, with their words kept: the ones the tutor may use.
    fn ticked(&self) -> Vec<&Source> {
        let Some(book) = &self.book else { return Vec::new() };
        let selected = book.selection();
        book.sources.sources.iter().filter(|s| selected.contains(&s.id) && s.status.has_words()).collect()
    }

    /// A source the tutor may use, or why not.
    fn usable(&self, id: &str) -> Result<&Source, String> {
        let book = self.book.as_ref().ok_or("the notebook cannot be read")?;
        let id = id.trim().trim_start_matches("[[").trim_end_matches("]]").split_whitespace().next().unwrap_or("");
        let Some(source) = book.sources.get(id) else {
            return Err(format!("there is no source {id}: notebook_sources lists them"));
        };
        if !book.selection().contains(&source.id) {
            return Err(format!("{id} is not ticked: it is not yours to use"));
        }
        if !source.status.has_words() {
            return Err(format!("{id} has not been read yet"));
        }
        Ok(source)
    }
}

/// What a role is called, as a sentence says it.
fn role(role: Role) -> &'static str {
    match role {
        Role::Notes => "notes",
        Role::Slides => "a lecture's slides",
        Role::Reading => "a reading",
        Role::Lab => "a lab or tutorial",
        Role::Assignment => "an assignment",
        Role::PastPaper => "a past exam paper",
        Role::Solution => "worked solutions",
        Role::OwnWork => "the student's own submitted work",
        Role::Recording => "a lecture's captions",
        Role::Other => "a source",
    }
}

fn extent(source: &Source) -> String {
    match source.units {
        // A folder read whole: its first lines list its files, and each
        // file's lines follow `==> path <==`, which its citations name.
        Units::Lines if source.kind == "folder" => format!("a folder of files read together, {} lines: its first lines list them, and each follows a line ==> its/path <==", source.count),
        Units::Pages => format!("{} pages", source.count),
        Units::Lines => format!("{} lines", source.count),
        Units::Seconds => format!("{} minutes", source.count.div_ceil(60)),
    }
}

pub fn tools() -> Value {
    crate::read_only(json!([
        {
            "name": "notebook_sources",
            "description": "List the sources of this notebook you may use — each one's id (S07), title, what it \
                            is to the course (notes, slides, a past exam paper, worked solutions…) and how long it \
                            is. Call it first. Only these sources may be used; cite them by id.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "search_sources",
            "description": "Search the notebook's sources for passages about something. Ask in the words a source \
                            would use: matching is lexical, so exact terms matter, and several searches with \
                            different words find more than one. Each result begins with its citation in double \
                            brackets, such as [[S07 p. 12]] or [[S03 § Search › Uniform cost L40-58]]: copy it \
                            exactly, brackets and all, after anything you take from that passage.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Terms to look for." },
                    "limit": { "type": "integer", "description": "How many passages; defaults to 6." },
                    "sources": { "type": "array", "items": { "type": "string" }, "description": "Search only these sources, by id. Omit for all." }
                },
                "required": ["query"]
            }
        },
        {
            "name": "read_source",
            "description": "Read a source in order: lines of a note, pages of a PDF, or minutes of a lecture's \
                            captions, from where you say. Use it to read around a passage a search found, or a \
                            short source whole. It hands back at most about 12,000 characters, and says where to \
                            go on from. Cite what you use as the text marks it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Its id: S07." },
                    "from": { "type": "integer", "description": "The first line, page or second; defaults to the start." },
                    "to": { "type": "integer", "description": "The last line, page or second." }
                },
                "required": ["source"]
            }
        },
        {
            "name": "view_pages",
            "description": "See pages of a PDF source as pictures — for slides whose words were too few to read, \
                            or a diagram or an equation a page's text cannot show. At most 4 pages a call. Cite \
                            what you take from a page as [[S07 p. 12]].",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Its id: S07." },
                    "pages": { "type": "array", "items": { "type": "integer" }, "description": "Page numbers, from 1." }
                },
                "required": ["source", "pages"]
            }
        },
        {
            "name": "notebook_status",
            "description": "How many sources this notebook has, how many you may use, and whether any is still being read.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ]))
}

/// Answer one tool call.
pub fn call(served: &mut Served, name: &str, args: &Value) -> Value {
    served.fresh();
    match name {
        "notebook_sources" => rpc::tool_output(sources(served), false),
        "search_sources" => {
            let (text, failed) = search(served, args);
            rpc::tool_output(text, failed)
        }
        "read_source" => match read(served, args) {
            Ok(text) => rpc::tool_output(text, false),
            Err(why) => rpc::tool_output(why, true),
        },
        "view_pages" => match view(served, args) {
            Ok(content) => json!({ "content": content, "isError": false }),
            Err(why) => rpc::tool_output(why, true),
        },
        "notebook_status" => rpc::tool_output(status(served), false),
        other => rpc::tool_output(format!("no tool named {other}"), true),
    }
}

fn sources(served: &Served) -> String {
    let Some(book) = &served.book else { return "the notebook cannot be read".into() };
    let ticked = served.ticked();
    if ticked.is_empty() {
        return "No sources are ticked for you to use. Say so: the student ticks sources in the notebook.".into();
    }
    let mut out = format!("The notebook \"{}\" — the sources you may use:\n", book.notebook.title);
    for s in &ticked {
        let thin = match s.thin.len() {
            0 => String::new(),
            _ => format!(" (pages {} have few words: view_pages shows them)", s.thin.iter().take(12).map(|p| p.to_string()).collect::<Vec<_>>().join(", ")),
        };
        out.push_str(&format!("{}  {} — {}, {}{thin}\n", s.id, s.title, role(s.role), extent(s)));
    }
    let others = book.sources.sources.len() - ticked.len();
    if others > 0 {
        out.push_str(&format!("{others} other source{} are not ticked, and are not yours to use.\n", if others == 1 { " is" } else { "s" }));
    }
    out
}

fn search(served: &Served, args: &Value) -> (String, bool) {
    let Some(query) = args.get("query").and_then(Value::as_str).filter(|q| !q.trim().is_empty()) else {
        return ("search_sources: `query` is required, and must be words to look for".into(), true);
    };
    let ticked: Vec<String> = served.ticked().iter().map(|s| s.id.clone()).collect();
    if ticked.is_empty() {
        return ("No sources are ticked for you to use.".into(), false);
    }
    let asked: Vec<String> = args.get("sources").and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_str).map(|s| s.trim().to_string()).collect()).unwrap_or_default();
    let within: Vec<String> = match asked.is_empty() {
        true => ticked,
        false => asked.into_iter().filter(|id| ticked.contains(id)).collect(),
    };
    if within.is_empty() {
        return ("none of those sources are ticked for you to use".into(), true);
    }
    let limit = args.get("limit").and_then(Value::as_u64).map_or(6, |n| n as usize).clamp(1, 20);
    let hits = served.library.search_in(query, limit, &Scope::of(within));
    if hits.is_empty() {
        return (format!("nothing in the sources about {query:?} — try other words, or say the sources do not cover it"), false);
    }
    (proton_library::quote(&hits), false)
}

/// A source's words as kept.
fn words(served: &Served, source: &Source) -> Result<Body, String> {
    let book = served.book.as_ref().ok_or("the notebook cannot be read")?;
    snapshot::words(book, &source.id, source.units).ok_or_else(|| format!("{}'s words are not kept", source.id))
}

fn read(served: &Served, args: &Value) -> Result<String, String> {
    let id = args.get("source").and_then(Value::as_str).ok_or("read_source: `source` is required")?;
    let source = served.usable(id)?;
    let from = args.get("from").and_then(Value::as_u64).map(|n| n as usize);
    let to = args.get("to").and_then(Value::as_u64).map(|n| n as usize);
    let id = &source.id;
    let mut out = String::new();
    let mut next = None;
    match words(served, source)? {
        Body::Lines(text) => {
            let lines: Vec<&str> = text.lines().collect();
            let first = from.unwrap_or(1).max(1);
            let last = to.unwrap_or(lines.len()).min(lines.len());
            let mut end = first;
            for n in first..=last {
                let line = lines[n - 1];
                if out.len() + line.len() > MOST && n > first {
                    next = Some(n);
                    break;
                }
                out.push_str(line);
                out.push('\n');
                end = n;
            }
            if first > lines.len() {
                return Err(format!("{id} has {} lines", lines.len()));
            }
            out = format!("[[{id} L{first}-{end}]]\n{out}");
            if let Some(n) = next.or((end < lines.len() && to.is_none()).then_some(end + 1)) {
                out.push_str(&format!("\n(It goes on at line {n} of {}.)", lines.len()));
            }
        }
        Body::Pages(pages) => {
            let first = from.unwrap_or(1).max(1);
            let last = to.unwrap_or(pages.len()).min(pages.len());
            if first > pages.len() {
                return Err(format!("{id} has {} pages", pages.len()));
            }
            for n in first..=last {
                let page = pages[n - 1].trim();
                if !out.is_empty() && out.len() + page.len() > MOST {
                    next = Some(n);
                    break;
                }
                let words = if page.is_empty() { "(no words were read on this page: view_pages shows it)" } else { page };
                out.push_str(&format!("[[{id} p. {n}]]\n{words}\n\n---\n\n"));
            }
            if let Some(n) = next {
                out.push_str(&format!("(It goes on at page {n} of {}.)", pages.len()));
            }
        }
        Body::Cues(cues) => {
            let first = from.unwrap_or(0) as f64;
            let last = to.map_or(f64::MAX, |t| t as f64);
            let mut shown = cues.iter().filter(|c| c.end >= first && c.start <= last).peekable();
            let start = shown.peek().map_or(0, |c| c.start.floor() as usize);
            let mut end = start;
            for cue in shown {
                if out.len() + cue.text.len() > MOST {
                    next = Some(cue.start.floor() as usize);
                    break;
                }
                out.push_str(&format!("{} {}\n", proton_library::chunk::clock(cue.start.floor() as usize), cue.text));
                end = cue.end.ceil() as usize;
            }
            out = format!("[[{id} @ {}-{}]]\n{out}", proton_library::chunk::clock(start), proton_library::chunk::clock(end));
            if let Some(n) = next {
                out.push_str(&format!("\n(It goes on at second {n}.)"));
            }
        }
    }
    Ok(out)
}

/// Where a source's file is, through the roots: only a file of one, or a
/// PDF fetched from the web and kept whole in the notebook.
fn file(served: &Served, source: &Source) -> Result<PathBuf, String> {
    if let Origin::Web { .. } = &source.origin {
        let kept = served.book.as_ref().and_then(|book| book.web_pdf(&source.id));
        return kept.ok_or_else(|| format!("{}'s PDF is not kept here: read_source reads its words", source.id));
    }
    let Origin::File { root, rel } = &source.origin else {
        return Err(format!("{} has no file to draw: it was pasted or written here", source.id));
    };
    let base = served.roots.iter().find(|(label, _)| label == root).map(|(_, path)| path).ok_or_else(|| format!("{}'s folder is not one this notebook can reach", source.id))?;
    if Path::new(rel).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err("not a path under its folder".into());
    }
    Ok(base.join(rel))
}

fn view(served: &Served, args: &Value) -> Result<Vec<Value>, String> {
    use base64::Engine as _;
    let id = args.get("source").and_then(Value::as_str).ok_or("view_pages: `source` is required")?;
    let source = served.usable(id)?;
    if source.kind != "pdf" {
        return Err(format!("{} is not a PDF: read_source reads it", source.id));
    }
    let mut pages: Vec<usize> = args.get("pages").and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_u64).map(|n| n as usize).collect()).unwrap_or_default();
    pages.retain(|p| (1..=source.count).contains(p));
    pages.dedup();
    if pages.is_empty() {
        return Err(format!("{} has pages 1 to {}", source.id, source.count));
    }
    pages.truncate(PAGES);
    let path = file(served, source)?;
    let meta = std::fs::symlink_metadata(&path).map_err(|_| format!("{}'s file is gone: read_source still reads its words", source.id))?;
    if proton_library::dataless::is_dataless(&meta) {
        return Err(format!("{}'s file is in iCloud: read_source reads its words", source.id));
    }
    let exe = pdftoppm().ok_or("pages cannot be drawn here: Poppler's pdftoppm is not installed")?;
    let mut content = vec![json!({ "type": "text", "text": format!("{} pages {}", source.id, pages.iter().map(|p| format!("[[{} p. {p}]]", source.id)).collect::<Vec<_>>().join(" ")) })];
    let out = std::env::temp_dir().join(format!("proton-pages-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&out);
    for page in pages {
        let stem = out.join(format!("p{page}"));
        let drawn = std::process::Command::new(&exe)
            .args(["-f", &page.to_string(), "-l", &page.to_string(), "-png", "-r", "110", "-singlefile"])
            .arg(&path)
            .arg(&stem)
            .output()
            .map_err(|e| e.to_string())?;
        let png = stem.with_extension("png");
        let bytes = std::fs::read(&png).map_err(|_| format!("page {page} could not be drawn: {}", String::from_utf8_lossy(&drawn.stderr).trim()))?;
        let _ = std::fs::remove_file(&png);
        content.push(json!({ "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(bytes), "mimeType": "image/png" }));
    }
    Ok(content)
}

/// Poppler's `pdftoppm`, where `pdftotext` is.
fn pdftoppm() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "pdftoppm.exe" } else { "pdftoppm" };
    proton_library::pdf::find_pdftotext().map(|t| t.with_file_name(exe)).filter(|p| p.is_file())
}

fn status(served: &Served) -> String {
    let Some(book) = &served.book else { return "the notebook cannot be read".into() };
    let ticked = served.ticked().len();
    let reading = book.sources.sources.iter().filter(|s| !s.status.has_words()).count();
    format!(
        "{} sources, {ticked} ticked for you to use, {} passages{}",
        book.sources.sources.len(),
        served.library.len(),
        if reading > 0 { format!("; {reading} not read yet") } else { String::new() }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proton_study::model::{SourceStatus, Units};
    use proton_study::Folder;

    /// A notebook of three sources — notes, slides, captions — two ticked.
    fn notebook(tag: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("proton-mcp-notebook-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = root.join("Courses");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("slides.pdf"), proton_library::pdf::fixture(&["Title", "Heuristics\nan admissible heuristic never overestimates the cost to the goal"])).unwrap();
        let mut book = Folder::new(root.join("Notebooks")).create("Search", 1).unwrap();
        let add = |book: &mut Book, title: &str, origin: Origin, body: Body, units: Units, count: usize, thin: Vec<usize>| {
            let id = book.sources.take_id();
            snapshot::keep(book, &id, &body).unwrap();
            book.sources.sources.push(Source {
                id,
                title: title.into(),
                kind: match units { Units::Pages => "pdf", Units::Seconds => "captions", Units::Lines => "markdown" }.into(),
                origin,
                role: Role::Notes,
                size: 1,
                mtime: 1,
                sha256: String::new(),
                units,
                count,
                thin,
                status: SourceStatus::Ready,
                added: 1,
                labels: Vec::new(),
            });
        };
        let lines = (1..=400).map(|n| format!("line {n} about breadth first search")).collect::<Vec<_>>().join("\n");
        add(&mut book, "Search.md", Origin::Pasted, Body::Lines(format!("# Search\n{lines}\n")), Units::Lines, 401, Vec::new());
        add(&mut book, "slides.pdf", Origin::File { root: "Courses".into(), rel: "slides.pdf".into() },
            Body::Pages(vec!["Title".into(), "Heuristics\nan admissible heuristic never overestimates the cost to the goal".into()]), Units::Pages, 2, vec![1]);
        add(&mut book, "solutions.md", Origin::Pasted, Body::Lines("# Answers\nthe secret answer is forty two\n".into()), Units::Lines, 2, Vec::new());
        book.save_sources().unwrap();
        book.select(&["S01".into(), "S02".into()]).unwrap();
        (book.dir().to_path_buf(), files)
    }

    fn text(out: &Value) -> String {
        out["content"][0]["text"].as_str().unwrap_or_default().to_string()
    }

    #[test]
    fn only_the_ticked_sources_are_listed_searched_and_read() {
        let (dir, files) = notebook("ticked");
        let mut served = Served::open(dir, vec![("Courses".into(), files)]);
        let listed = text(&call(&mut served, "notebook_sources", &json!({})));
        assert!(listed.contains("S01  Search.md") && listed.contains("S02  slides.pdf"), "{listed}");
        assert!(listed.contains("pages 1 have few words"), "{listed}");
        assert!(!listed.contains("solutions"), "an unticked source is named: {listed}");
        let found = text(&call(&mut served, "search_sources", &json!({ "query": "admissible heuristic" })));
        assert!(found.starts_with("[[S02 § Heuristics p. 2]]"), "{found}");
        let hidden = text(&call(&mut served, "search_sources", &json!({ "query": "secret answer forty two" })));
        assert!(!hidden.contains("[[S03"), "an unticked source was searched: {hidden}");
        let refused = call(&mut served, "read_source", &json!({ "source": "S03" }));
        assert_eq!(refused["isError"], json!(true));
        assert!(text(&refused).contains("not ticked"));
    }

    #[test]
    fn reading_goes_in_order_and_says_where_it_goes_on() {
        let (dir, files) = notebook("read");
        let mut served = Served::open(dir, vec![("Courses".into(), files)]);
        let first = text(&call(&mut served, "read_source", &json!({ "source": "S01" })));
        assert!(first.starts_with("[[S01 L1-"), "{}", &first[..40]);
        assert!(first.contains("It goes on at line"), "a long source is read in parts");
        assert!(first.len() < MOST + 400);
        let part = text(&call(&mut served, "read_source", &json!({ "source": "S01", "from": 10, "to": 12 })));
        assert!(part.starts_with("[[S01 L10-12]]\nline 9 about"), "{part}");
        let pages = text(&call(&mut served, "read_source", &json!({ "source": "S02" })));
        assert!(pages.contains("[[S02 p. 1]]\nTitle") && pages.contains("[[S02 p. 2]]"), "{pages}");
    }

    #[test]
    fn ticking_in_the_window_is_seen_at_the_next_call() {
        let (dir, files) = notebook("tick");
        let mut served = Served::open(dir.clone(), vec![("Courses".into(), files)]);
        assert!(!text(&call(&mut served, "search_sources", &json!({ "query": "forty two" }))).contains("[[S03"));
        Book::read(&dir).unwrap().select(&["S03".into()]).unwrap();
        assert!(text(&call(&mut served, "search_sources", &json!({ "query": "forty two" }))).contains("[[S03 § Answers"));
    }

    #[test]
    fn a_page_is_drawn_from_its_file_and_never_from_one_out_of_reach() {
        let (dir, files) = notebook("view");
        let mut served = Served::open(dir.clone(), vec![("Courses".into(), files.clone())]);
        let out = call(&mut served, "view_pages", &json!({ "source": "S02", "pages": [1, 9] }));
        if pdftoppm().is_some() {
            assert_eq!(out["isError"], json!(false), "{out}");
            assert_eq!(out["content"][1]["type"], json!("image"));
            assert_eq!(out["content"].as_array().unwrap().len(), 2, "page 9 is not a page");
        }
        let notes = call(&mut served, "view_pages", &json!({ "source": "S01", "pages": [1] }));
        assert!(text(&notes).contains("not a PDF"));
        // With no root to find it under, no file is drawn.
        let mut blind = Served::open(dir, Vec::new());
        assert!(text(&call(&mut blind, "view_pages", &json!({ "source": "S02", "pages": [1] }))).contains("not one this notebook can reach"));
    }

    #[test]
    fn a_pdf_fetched_from_the_web_is_drawn_from_the_copy_the_notebook_keeps() {
        let (dir, _) = notebook("web");
        let mut book = Book::read(&dir).unwrap();
        let id = book.sources.take_id();
        let pages = ["Softmax", "the softmax of a vector is a probability distribution"];
        snapshot::keep(&book, &id, &Body::Pages(pages.iter().map(|p| p.to_string()).collect())).unwrap();
        book.sources.sources.push(Source {
            id: id.clone(),
            title: "softmax.pdf".into(),
            kind: "pdf".into(),
            origin: Origin::Web { url: "https://example.com/softmax.pdf".into(), fetched: 1 },
            role: Role::Reading,
            size: 1,
            mtime: 1,
            sha256: String::new(),
            units: Units::Pages,
            count: 2,
            thin: Vec::new(),
            status: SourceStatus::Ready,
            added: 1,
            labels: Vec::new(),
        });
        book.save_sources().unwrap();
        book.select(std::slice::from_ref(&id)).unwrap();
        let mut served = Served::open(dir.clone(), Vec::new());
        let missing = call(&mut served, "view_pages", &json!({ "source": id, "pages": [1] }));
        assert!(text(&missing).contains("not kept here"), "{missing}");
        std::fs::create_dir_all(dir.join(".notebook/web")).unwrap();
        std::fs::write(dir.join(format!(".notebook/web/{id}.pdf")), proton_library::pdf::fixture(&pages)).unwrap();
        if pdftoppm().is_some() {
            let out = call(&mut served, "view_pages", &json!({ "source": id, "pages": [2] }));
            assert_eq!(out["isError"], json!(false), "{out}");
            assert_eq!(out["content"][1]["type"], json!("image"));
        }
    }
}
