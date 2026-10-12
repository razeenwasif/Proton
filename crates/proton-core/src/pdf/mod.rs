//! PDF: the person's PDFs, read and edited in tabs, whichever project is
//! open. The session's, as Notebook is — a PDF is the person's wherever it
//! lives, and opening one is not opening a project.
//!
//! The window draws a tab from bytes the core holds and names by a key,
//! `pdfview:<tab>:<version>`, which proton-shell's `read_document` asks the
//! session for. A view is never written to the disk, so the pages of a file
//! opened with its password never are either.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::folder;
use crate::workspace::notice;

mod agent;
pub mod library;
pub use agent::TOOLS;

/// The largest PDF opened: the most the shell reads for the page.
pub const LARGEST: u64 = 128 * 1024 * 1024;

/// How many files opened before are offered again.
const RECENT: usize = 20;

/// How a view's key begins.
const VIEW: &str = "pdfview:";

/// Whether a command is PDF's.
pub fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::PdfOpen { .. }
            | Command::PdfClose { .. }
            | Command::PdfFocus { .. }
            | Command::PdfUndo
            | Command::PdfRedo
            | Command::PdfSave
            | Command::PdfSaveAs { .. }
            | Command::PdfRotate { .. }
            | Command::PdfDelete { .. }
            | Command::PdfInsertBlank { .. }
            | Command::PdfMove { .. }
            | Command::PdfInsertFile { .. }
            | Command::PdfExtract { .. }
            | Command::PdfSplit { .. }
            | Command::PdfCombine { .. }
            | Command::PdfComment { .. }
            | Command::PdfReply { .. }
            | Command::PdfEditComment { .. }
            | Command::PdfDeleteComment { .. }
            | Command::PdfSetField { .. }
            | Command::PdfFlatten
            | Command::PdfSetInfo { .. }
            | Command::PdfProtect { .. }
            | Command::PdfUnprotect
            | Command::PdfSanitize
            | Command::PdfCompress
            | Command::PdfOcr { .. }
            | Command::PdfOcrStop
            | Command::PdfOcrLangs
            | Command::PdfAt { .. }
            | Command::PdfLibraryScan
            | Command::PdfLibraryFolders { .. }
            | Command::PdfCovers { .. }
    )
}

/// A tab, as the window draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "PdfTab")]
#[serde(rename_all = "camelCase")]
pub struct TabView {
    pub id: u32,
    pub name: String,
    /// Where the file is, as the person would write it: `~/Papers/x.pdf`.
    pub path: String,
    /// Changed since it was opened or saved.
    pub dirty: bool,
    /// Why it can only be read, when it can.
    pub read_only: Option<String>,
    /// Saved with a password.
    pub protected: bool,
    /// What undo and redo would do: "turn page 2".
    pub undo: Option<String>,
    pub redo: Option<String>,
}

/// One open PDF.
#[derive(Debug)]
struct Tab {
    id: u32,
    path: PathBuf,
    name: String,
    /// What the window draws.
    view: Arc<Vec<u8>>,
    /// Goes up with each change, and names the view.
    version: u64,
    /// What is changed and saved; `None` for a file only read, with why.
    doc: Result<Box<proton_pdf::Doc>, String>,
    /// The page the person is reading, from one, as the window last said.
    at: u32,
    /// The text of the pages the agent has read, and the version it is of.
    read: (u64, std::collections::HashMap<u32, proton_pdf::text::PageText>),
}

impl Tab {
    fn key(&self) -> String {
        format!("{VIEW}{}:{}", self.id, self.version)
    }

    fn view(&self) -> TabView {
        let doc = self.doc.as_ref().ok();
        TabView {
            id: self.id,
            name: self.name.clone(),
            path: folder::tilde(&self.path),
            dirty: doc.is_some_and(|d| d.dirty()),
            read_only: self.doc.as_ref().err().cloned(),
            protected: doc.is_some_and(|d| d.protected()),
            undo: doc.and_then(|d| d.undoable()),
            redo: doc.and_then(|d| d.redoable()),
        }
    }

    fn dirty(&self) -> bool {
        self.doc.as_ref().is_ok_and(|d| d.dirty())
    }
}

/// The open PDFs, and the ones opened before.
#[derive(Debug, Default)]
pub struct Pdfs {
    tabs: Vec<Tab>,
    active: Option<u32>,
    next: u32,
    /// Opened before, the last first.
    recent: Vec<PathBuf>,
    /// Where the files opened before are kept; `None` in tests.
    store: Option<PathBuf>,
    /// The PDFs in the project on screen, as it names them, and its root.
    project: (Vec<String>, PathBuf),
    /// Comments made here so far, for naming the next one.
    made: u64,
    /// The programs OCR uses, where the machine has them; none in tests
    /// unless they are given stand-ins.
    ocr: Option<proton_lab::ocr::OcrTools>,
    /// The languages they read, and the folder tesseract's data is in, once
    /// asked.
    langs: Option<(Vec<String>, Option<PathBuf>)>,
    /// Pages being read, and the tab they are of.
    job: Option<(u32, proton_lab::ocr::OcrJob)>,
    /// How far they had got when the window was last told.
    told: (usize, usize),
    /// The books in the folders the person keeps them in.
    library: library::Library,
    /// Where each PDF was left, and when: kept with those opened before.
    read: library::Read,
    /// The person's own settings, where the library's folders are kept.
    person: Option<PathBuf>,
}

/// What is kept between sessions: the files opened before, and where each
/// PDF was left. Once only the first, as a list.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Kept {
    #[serde(default)]
    recent: Vec<PathBuf>,
    #[serde(default)]
    read: Vec<(PathBuf, u32, u64)>,
}

impl Pdfs {
    /// Nothing open, nothing kept: for tests.
    pub fn new() -> Pdfs {
        Pdfs::default()
    }

    /// With these programs for OCR: the machine's, or stand-ins in a test.
    pub fn with_ocr(mut self, tools: Option<proton_lab::ocr::OcrTools>) -> Pdfs {
        self.ocr = tools;
        self
    }

    /// The files opened before, kept in `file`.
    pub fn kept_in(file: PathBuf) -> Pdfs {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let kept = serde_json::from_str::<Vec<PathBuf>>(&text)
            .map(|recent| Kept { recent, read: Vec::new() })
            .or_else(|_| serde_json::from_str::<Kept>(&text))
            .unwrap_or_default();
        let recent = kept.recent.into_iter().filter(|p| p.is_file()).collect();
        let read = kept.read.into_iter().map(|(path, page, at)| (path, (page, at))).collect();
        let mut library = file.parent().map(library::Library::kept_in).unwrap_or_default();
        library.drawn_by(proton_library::pdf::find_pdftoppm());
        Pdfs { recent, read, store: Some(file), library, ..Pdfs::default() }
    }

    /// The library's folders from the person's own settings, where changes
    /// to them are written: `~/Documents/Books` when they name none and it
    /// is there.
    pub fn with_person_settings(mut self, file: &Path) -> Pdfs {
        let settings = crate::settings::Settings::at(file.to_path_buf());
        let named: Option<Vec<String>> = settings
            .get("pdf.library")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(str::to_string).collect());
        let folders = named.unwrap_or_else(|| {
            let books = "~/Documents/Books";
            if folder::expand(books).is_dir() { vec![books.to_string()] } else { Vec::new() }
        });
        self.library.set_folders(folders);
        self.person = Some(file.to_path_buf());
        self
    }

    /// A library of these folders, its covers drawn by `covers`: for a test.
    pub fn with_library(mut self, folders: Vec<String>, covers: Option<PathBuf>) -> Pdfs {
        self.library.set_folders(folders);
        self.library.drawn_by(covers);
        self
    }

    /// The library as it is, each book where it was left.
    fn library_event(&self) -> Event {
        self.library.event(&self.read)
    }

    /// Set the library's folders, keep them in the person's settings, and
    /// walk them.
    fn set_library(&mut self, folders: Vec<String>) -> Vec<Event> {
        let folders: Vec<String> = folders.into_iter().map(|f| f.trim().trim_end_matches(['/', '\\']).to_string()).filter(|f| !f.is_empty()).collect();
        let mut events: Vec<Event> = folders
            .iter()
            .filter(|typed| !folder::expand(typed).is_dir())
            .map(|typed| notice(format!("{typed}: no such folder"), NoticeLevel::Warn))
            .collect();
        if let Some(file) = &self.person {
            if let Err(why) = crate::settings::Settings::at(file.clone()).set("pdf.library", serde_json::Value::from(folders.clone())) {
                events.push(notice(format!("The library's folders could not be saved: {why}"), NoticeLevel::Warn));
            }
        }
        self.library.set_folders(folders);
        self.library.walk();
        events.push(self.library_event());
        events
    }

    /// The mode entered, over the project at `root`, whose PDFs are `pdfs`:
    /// the tabs, and the one on top drawn.
    pub fn enter(&mut self, root: &Path, pdfs: Vec<String>) -> Vec<Event> {
        self.project = (pdfs, root.to_path_buf());
        // The library as it was kept, while it is walked again.
        if !self.library.walked() && !self.library.folders().is_empty() {
            self.library.walk();
        }
        let mut events = vec![self.tabs_event(), self.library_event()];
        if let Some(tab) = self.active_tab() {
            events.push(view_event(tab, None, false));
            events.push(comments_event(tab));
            events.push(fields_event(tab));
            events.push(info_event(tab));
        }
        events
    }

    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::PdfOpen { path, page, password } => self.open_with(&path, page, password.as_ref().map(|p| p.0.as_str())),
            Command::PdfClose { tab, discard } => self.close(tab, discard),
            Command::PdfFocus { tab } => self.focus(tab),
            Command::PdfUndo => self.history(false),
            Command::PdfRedo => self.history(true),
            Command::PdfSave => self.save(None),
            Command::PdfSaveAs { path } => self.save(Some(&path)),
            Command::PdfRotate { pages, by } => self.change(proton_pdf::Op::Rotate { pages, by }),
            Command::PdfDelete { pages } => self.change(proton_pdf::Op::Delete { pages }),
            Command::PdfInsertBlank { after, count } => self.change(proton_pdf::Op::InsertBlank { after, count }),
            Command::PdfMove { pages, before } => self.change(proton_pdf::Op::Move { pages, before }),
            Command::PdfInsertFile { after, path, pages } => match self.located(&path).and_then(|p| read(&p).map(|b| (p, b))) {
                Ok((path, bytes)) => self.change(proton_pdf::Op::InsertFrom { after, name: folder::name_of(&path), bytes: Arc::new(bytes), pages }),
                Err(why) => vec![notice(why, NoticeLevel::Error)],
            },
            Command::PdfExtract { pages, path } => self.extract(&pages, &path),
            Command::PdfSplit { every, at } => self.split(every, &at),
            Command::PdfCombine { paths, path } => self.combine(&paths, &path),
            Command::PdfComment { page, mark } => {
                let (author, date, name) = self.signed();
                self.change(proton_pdf::Op::Comment { page, mark, author, date, name })
            }
            Command::PdfReply { id, text } => {
                let (author, date, name) = self.signed();
                self.change(proton_pdf::Op::Reply { to: id, text, author, date, name })
            }
            Command::PdfEditComment { id, color, text } => {
                let (_, date, _) = self.signed();
                self.change(proton_pdf::Op::EditComment { id, color, text, date })
            }
            Command::PdfDeleteComment { id } => self.change(proton_pdf::Op::DeleteComment { id }),
            Command::PdfSetField { name, value } => self.change_quietly(proton_pdf::Op::SetField { name, value }),
            Command::PdfFlatten => self.change(proton_pdf::Op::Flatten),
            Command::PdfSetInfo { title, author, subject, keywords } => {
                let (_, date, _) = self.signed();
                self.change(proton_pdf::Op::SetInfo { title, author, subject, keywords, date })
            }
            Command::PdfProtect { protection } => self.change(proton_pdf::Op::Protect { protection }),
            Command::PdfUnprotect => self.change(proton_pdf::Op::Unprotect),
            Command::PdfSanitize => self.change(proton_pdf::Op::Sanitize),
            Command::PdfCompress => self.change(proton_pdf::Op::Compress),
            Command::PdfOcr { pages, langs } => self.ocr(&pages, &langs),
            Command::PdfOcrStop => {
                if let Some((_, job)) = &mut self.job {
                    job.stop();
                }
                Vec::new()
            }
            Command::PdfOcrLangs => vec![Event::PdfOcrLangs { langs: self.languages().0 }],
            Command::PdfAt { tab, page } => {
                let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab) else { return Vec::new() };
                tab.at = page.max(1);
                let path = tab.path.clone();
                if self.read.get(&path).map(|r| r.0) != Some(page.max(1)) {
                    self.read.insert(path, (page.max(1), now()));
                    self.keep();
                }
                Vec::new()
            }
            Command::PdfLibraryScan => {
                self.library.walk();
                vec![self.library_event()]
            }
            Command::PdfLibraryFolders { folders } => self.set_library(folders),
            Command::PdfCovers { paths } => match self.library.covers(&paths) {
                covers if covers.is_empty() => Vec::new(),
                covers => vec![Event::PdfCovers { covers }],
            },
            _ => Vec::new(),
        }
    }

    /// The bytes a view's key names, while its tab is open.
    pub fn view_bytes(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        let rest = key.strip_prefix(VIEW)?;
        let (id, version) = rest.split_once(':')?;
        let (id, version): (u32, u64) = (id.parse().ok()?, version.parse().ok()?);
        self.tabs.iter().find(|t| t.id == id && t.version == version).map(|t| t.view.clone())
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| Some(t.id) == self.active)
    }

    fn tabs_event(&self) -> Event {
        let (project, root) = &self.project;
        Event::PdfTabs {
            tabs: self.tabs.iter().map(Tab::view).collect(),
            active: self.active,
            recent: self.recent.iter().map(|p| folder::tilde(p)).collect(),
            project: project.clone(),
            root: root.display().to_string(),
        }
    }

    /// Open a PDF — typed as a path is, `~` and all, or relative to the
    /// project on screen — or bring it forward if it is open.
    fn open(&mut self, typed: &str, page: Option<u32>) -> Vec<Event> {
        self.open_with(typed, page, None)
    }

    /// The same, with the password it is protected by.
    fn open_with(&mut self, typed: &str, page: Option<u32>, password: Option<&str>) -> Vec<Event> {
        let typed = typed.trim();
        if typed.is_empty() {
            return vec![notice("Say which PDF to open.".into(), NoticeLevel::Info)];
        }
        let expanded = folder::expand(typed);
        let path = if expanded.is_absolute() { expanded } else { self.project.1.join(expanded) };
        let path = folder::normalise(&path);
        if let Some(tab) = self.tabs.iter().find(|t| t.path == path) {
            self.active = Some(tab.id);
            return vec![self.tabs_event(), view_event(tab, page, false), comments_event(tab), fields_event(tab), info_event(tab)];
        }
        let bytes = match read(&path) {
            Ok(bytes) => bytes,
            Err(why) => return vec![notice(why, NoticeLevel::Error)],
        };
        let file = Arc::new(bytes);
        let doc = match proton_pdf::Doc::open_with(file.clone(), password) {
            proton_pdf::Opened::Editable(doc) => Ok(doc),
            proton_pdf::Opened::ReadOnly(why) => Err(why),
            proton_pdf::Opened::Locked { wrong } => {
                return vec![Event::PdfPassword { path: path.display().to_string(), name: folder::name_of(&path), wrong }];
            }
        };
        // What the window draws: the file — or, opened with its password,
        // the document decrypted, which is held in memory only.
        let view = match doc.as_ref().map(|d| d.view_bytes()) {
            Ok(Ok(bytes)) if doc.as_ref().is_ok_and(|d| d.protected()) => Arc::new(bytes),
            _ => file,
        };
        self.next += 1;
        // Where it was left, unless a page is asked for.
        let page = page.or_else(|| self.read.get(&path).map(|r| r.0).filter(|p| *p > 1));
        let at = page.unwrap_or(1);
        self.read.insert(path.clone(), (at, now()));
        let tab = Tab { id: self.next, name: folder::name_of(&path), path: path.clone(), view, version: 1, doc, at, read: Default::default() };
        self.active = Some(tab.id);
        let view = view_event(&tab, page, false);
        let comments = comments_event(&tab);
        let fields = fields_event(&tab);
        let info = info_event(&tab);
        self.tabs.push(tab);
        self.remember(&path);
        vec![self.tabs_event(), view, comments, fields, info]
    }

    fn close(&mut self, id: u32, discard: bool) -> Vec<Event> {
        let Some(at) = self.tabs.iter().position(|t| t.id == id) else { return Vec::new() };
        if self.tabs[at].dirty() && !discard {
            return vec![notice(format!("{} has changes not saved: save it, or close it letting them go.", self.tabs[at].name), NoticeLevel::Warn)];
        }
        self.tabs.remove(at);
        if self.active == Some(id) {
            // The one beside it comes forward, as a browser's tab does.
            self.active = self.tabs.get(at).or_else(|| self.tabs.last()).map(|t| t.id);
        }
        let mut events = vec![self.tabs_event()];
        if let Some(tab) = self.active_tab() {
            events.push(view_event(tab, None, false));
            events.push(comments_event(tab));
            events.push(fields_event(tab));
            events.push(info_event(tab));
        }
        // How far it was read, on its shelf.
        events.push(self.library_event());
        events
    }

    fn focus(&mut self, id: u32) -> Vec<Event> {
        let Some(tab) = self.tabs.iter().find(|t| t.id == id) else { return Vec::new() };
        self.active = Some(id);
        vec![self.tabs_event(), view_event(tab, None, false), comments_event(tab), fields_event(tab), info_event(tab)]
    }

    /// The tab on top, to change; or why it cannot be.
    fn editable(&mut self) -> Result<&mut Tab, String> {
        let id = self.active.ok_or("Open a PDF first.")?;
        let tab = self.tabs.iter_mut().find(|t| t.id == id).ok_or("Open a PDF first.")?;
        if let Err(why) = &tab.doc {
            return Err(format!("{} can only be read: {why}.", tab.name));
        }
        Ok(tab)
    }

    /// The tab on top drawn again, as its document now is.
    fn redrawn(&mut self) -> Vec<Event> {
        self.redrawn_as(false)
    }

    /// The same; `quiet` when the window has it drawn already — a field
    /// typed in — and only the view's name moves on.
    fn redrawn_as(&mut self, quiet: bool) -> Vec<Event> {
        let Some(id) = self.active else { return Vec::new() };
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else { return Vec::new() };
        let bytes = match tab.doc.as_ref().map(|d| d.view_bytes()) {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(why)) => return vec![notice(why, NoticeLevel::Error)],
            Err(_) => return Vec::new(),
        };
        tab.view = Arc::new(bytes);
        tab.version += 1;
        let view = view_event(tab, None, quiet);
        let comments = comments_event(tab);
        let fields = fields_event(tab);
        let info = info_event(tab);
        vec![self.tabs_event(), view, comments, fields, info]
    }

    /// Make a change to the tab on top: held until it is saved.
    fn change(&mut self, op: proton_pdf::Op) -> Vec<Event> {
        let done = match self.editable() {
            Ok(tab) => tab.doc.as_mut().map_err(|e| e.clone()).and_then(|d| d.apply(op)),
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        match done {
            Ok(()) => self.redrawn(),
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    /// A change the window has drawn already: a field filled in.
    fn change_quietly(&mut self, op: proton_pdf::Op) -> Vec<Event> {
        let done = match self.editable() {
            Ok(tab) => tab.doc.as_mut().map_err(|e| e.clone()).and_then(|d| d.apply(op)),
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        match done {
            Ok(()) => self.redrawn_as(true),
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    /// Undo, or redo, the last change to the tab on top.
    fn history(&mut self, redo: bool) -> Vec<Event> {
        let done = match self.editable() {
            Ok(tab) => tab.doc.as_mut().map_err(|e| e.clone()).and_then(|d| if redo { d.redo() } else { d.undo() }),
            Err(why) => return vec![notice(why, NoticeLevel::Info)],
        };
        match done {
            Ok(Some(_)) => self.redrawn(),
            Ok(None) => vec![notice(if redo { "Nothing to redo." } else { "Nothing to undo." }.into(), NoticeLevel::Info)],
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    /// Save the tab on top where it is, or as another file.
    fn save(&mut self, typed: Option<&str>) -> Vec<Event> {
        let target = match typed.map(str::trim) {
            None => None,
            Some("") => return vec![notice("Say where to save it.".into(), NoticeLevel::Info)],
            Some(t) => {
                let path = folder::expand(t);
                let path = if path.is_absolute() { path } else { self.project.1.join(path) };
                if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
                    return vec![notice(format!("{}: a PDF's name ends in .pdf", folder::tilde(&path)), NoticeLevel::Info)];
                }
                if self.tabs.iter().any(|t| t.path == path && Some(t.id) != self.active) {
                    return vec![notice(format!("{} is open in another tab.", folder::tilde(&path)), NoticeLevel::Warn)];
                }
                Some(path)
            }
        };
        let saved = match self.editable() {
            Ok(tab) => {
                let path = target.clone().unwrap_or_else(|| tab.path.clone());
                match tab.doc.as_mut().map_err(|e| e.clone()).and_then(|d| d.save(&path)) {
                    Ok(()) => {
                        if target.is_some() {
                            tab.name = folder::name_of(&path);
                            tab.path = folder::normalise(&path);
                        }
                        Ok(tab.path.clone())
                    }
                    Err(why) => Err(why),
                }
            }
            Err(why) => Err(why),
        };
        match saved {
            Ok(path) => {
                if target.is_some() {
                    self.remember(&path);
                }
                let mut events = vec![notice(format!("saved {}", folder::tilde(&path)), NoticeLevel::Info)];
                events.extend(self.redrawn());
                events
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    /// Who a comment is by, when, and a name for it no other has: the
    /// person, as their account names them.
    fn signed(&mut self) -> (String, String, String) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.made += 1;
        let author = std::env::var("USER").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| "Proton".into());
        (author, proton_pdf::annot::date(secs), format!("proton-{secs}-{}", self.made))
    }

    /// Where a path typed — `~` and all, or relative to the project on
    /// screen — is.
    fn located(&self, typed: &str) -> Result<PathBuf, String> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Err("Say which file.".into());
        }
        let path = folder::expand(typed);
        Ok(folder::normalise(&if path.is_absolute() { path } else { self.project.1.join(path) }))
    }

    /// Where a new PDF is to be written: not over anything there.
    fn fresh(&self, typed: &str) -> Result<PathBuf, String> {
        let path = self.located(typed)?;
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            return Err(format!("{}: a PDF's name ends in .pdf", folder::tilde(&path)));
        }
        if path.exists() {
            return Err(format!("{} is there already: choose another name.", folder::tilde(&path)));
        }
        if !path.parent().is_some_and(Path::is_dir) {
            return Err(format!("{}: no such folder", folder::tilde(path.parent().unwrap_or(&path))));
        }
        Ok(path)
    }

    /// The tab on top's document as it is now, and its file.
    fn current(&self) -> Result<(&proton_pdf::Doc, PathBuf), String> {
        let id = self.active.ok_or("Open a PDF first.")?;
        let tab = self.tabs.iter().find(|t| t.id == id).ok_or("Open a PDF first.")?;
        match &tab.doc {
            Ok(doc) => Ok((doc, tab.path.clone())),
            Err(why) => Err(format!("{} can only be read: {why}.", tab.name)),
        }
    }

    /// Some pages of the tab on top, as it is now, written as a new PDF —
    /// then opened.
    fn extract(&mut self, pages: &[u32], typed: &str) -> Vec<Event> {
        match self.extract_to(pages, typed) {
            Ok(path) => {
                let mut events = vec![notice(format!("wrote {}", folder::tilde(&path)), NoticeLevel::Info)];
                events.extend(self.open(&path.display().to_string(), None));
                events
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    /// The same, written: where.
    fn extract_to(&mut self, pages: &[u32], typed: &str) -> Result<PathBuf, String> {
        let path = self.fresh(typed)?;
        let (doc, _) = self.current()?;
        let bytes = proton_pdf::combine::extract(doc.document(), pages)?;
        proton_pdf::save::atomically(&path, &bytes)?;
        Ok(path)
    }

    /// The tab on top split into PDFs beside it, `name-1.pdf` and on.
    fn split(&mut self, every: Option<u32>, at: &[u32]) -> Vec<Event> {
        let made = (|| {
            let (doc, path) = self.current()?;
            let ranges = proton_pdf::combine::ranges(doc.pages(), every, at)?;
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "part".into());
            let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let targets: Vec<PathBuf> = (1..=ranges.len()).map(|i| dir.join(format!("{stem}-{i}.pdf"))).collect();
            if let Some(there) = targets.iter().find(|t| t.exists()) {
                return Err(format!("{} is there already: nothing was split.", folder::tilde(there)));
            }
            for (range, target) in ranges.iter().zip(&targets) {
                let bytes = proton_pdf::combine::extract(doc.document(), range)?;
                proton_pdf::save::atomically(target, &bytes)?;
            }
            Ok(targets)
        })();
        match made {
            Ok(targets) => vec![notice(
                format!("split into {}: {}", targets.len(), targets.iter().map(|t| folder::name_of(t)).collect::<Vec<_>>().join(", ")),
                NoticeLevel::Info,
            )],
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    /// PDFs combined into a new one, then opened. One open with changes is
    /// combined as it is now.
    fn combine(&mut self, paths: &[String], typed: &str) -> Vec<Event> {
        let made = (|| {
            let target = self.fresh(typed)?;
            let mut parts = Vec::new();
            for typed in paths {
                let path = self.located(typed)?;
                let open = self.tabs.iter().find(|t| t.path == path).and_then(|t| t.doc.as_ref().ok());
                let bytes = match open {
                    Some(doc) => doc.bytes()?,
                    None => read(&path)?,
                };
                parts.push((folder::name_of(&path), bytes));
            }
            let bytes = proton_pdf::combine::combine(&parts)?;
            proton_pdf::save::atomically(&target, &bytes)?;
            Ok(target)
        })();
        match made {
            Ok(path) => {
                let mut events = vec![notice(format!("combined {} into {}", paths.len(), folder::tilde(&path)), NoticeLevel::Info)];
                events.extend(self.open(&path.display().to_string(), None));
                events
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    /// The languages OCR reads here, asked of tesseract once.
    fn languages(&mut self) -> (Vec<String>, Option<PathBuf>) {
        if self.langs.is_none() {
            self.langs = Some(self.ocr.as_ref().map(|t| t.languages()).unwrap_or_default());
        }
        self.langs.clone().unwrap_or_default()
    }

    /// Start reading `pages` of the tab on top with OCR.
    fn ocr(&mut self, pages: &[u32], langs: &[String]) -> Vec<Event> {
        let Some(tools) = self.ocr.clone() else {
            let tab = self.active.unwrap_or(0);
            return vec![Event::PdfOcr { tab, state: "missing".into(), done: 0, total: 0, said: "OCR needs tesseract and Poppler's pdftoppm: brew install tesseract poppler".into() }];
        };
        if self.job.as_ref().is_some_and(|(_, j)| j.state() == proton_lab::run::State::Running) {
            return vec![notice("Pages are being read already: stop that first.".into(), NoticeLevel::Info)];
        }
        let (_, data) = self.languages();
        let started = match self.editable() {
            Ok(tab) => {
                let id = tab.id;
                let count = tab.doc.as_ref().map(|d| d.pages()).unwrap_or(0);
                if let Some(bad) = pages.iter().find(|n| **n == 0 || **n > count) {
                    return vec![notice(format!("there is no page {bad}"), NoticeLevel::Warn)];
                }
                // What is read is the document as it is now, decrypted: in
                // a folder of the job's own, which goes when it does.
                match tab.doc.as_ref().map_err(|e| e.clone()).and_then(|d| d.view_bytes()) {
                    Ok(bytes) => {
                        let reads: Vec<PathBuf> = data.into_iter().collect();
                        proton_lab::ocr::OcrJob::start(&tools, &bytes, pages, langs, &std::env::temp_dir(), proton_lab::sandbox::available(), &reads).map(|job| (id, job))
                    }
                    Err(why) => Err(why),
                }
            }
            Err(why) => Err(why),
        };
        match started {
            Ok((id, job)) => {
                let (done, total) = job.progress();
                self.told = (done, total);
                self.job = Some((id, job));
                vec![Event::PdfOcr { tab: id, state: "running".into(), done: done as u32, total: total as u32, said: String::new() }]
            }
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    /// Pages being read: how far they have got, and once read, laid over
    /// their pages — one change, to undo like any other.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let (changed, covers) = self.library.poll();
        if changed {
            events.push(self.library_event());
        }
        if !covers.is_empty() {
            events.push(Event::PdfCovers { covers });
        }
        events.extend(self.poll_ocr());
        events
    }

    /// The same, of pages being read with OCR.
    fn poll_ocr(&mut self) -> Vec<Event> {
        let Some((id, job)) = &mut self.job else { return Vec::new() };
        let id = *id;
        if !job.poll() {
            let (done, total) = job.progress();
            if self.told == (done, total) {
                return Vec::new();
            }
            self.told = (done, total);
            return vec![Event::PdfOcr { tab: id, state: "running".into(), done: done as u32, total: total as u32, said: String::new() }];
        }
        let (_, job) = self.job.take().expect("a job");
        let (done, total) = job.progress();
        use proton_lab::run::State;
        let state = match job.state() {
            State::Done => "done",
            State::Stopped => "stopped",
            State::TimedOut => "timedOut",
            _ => "failed",
        };
        let mut events = vec![Event::PdfOcr { tab: id, state: state.into(), done: done as u32, total: total as u32, said: job.said().to_string() }];
        if job.state() != State::Done {
            return events;
        }
        let read: Vec<(u32, Arc<Vec<u8>>)> = job.read().into_iter().map(|(n, b)| (n, Arc::new(b))).collect();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else { return events };
        let Ok(doc) = tab.doc.as_mut() else { return events };
        if let Err(why) = doc.apply(proton_pdf::Op::Ocr { pages: read }) {
            events.push(notice(why, NoticeLevel::Error));
            return events;
        }
        if self.active == Some(id) {
            events.extend(self.redrawn());
        } else if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            // Drawn when it comes forward.
            if let Ok(Ok(bytes)) = tab.doc.as_ref().map(|d| d.view_bytes()) {
                tab.view = Arc::new(bytes);
                tab.version += 1;
            }
        }
        events
    }

    /// Reading pages stopped: the window is going.
    pub fn shutdown(&mut self) {
        self.job = None;
    }

    /// The tabs with changes not saved, by name.
    pub fn unsaved(&self) -> Vec<String> {
        self.tabs.iter().filter(|t| t.dirty()).map(|t| t.name.clone()).collect()
    }

    /// Save every tab with changes; the ones that could not be, and why.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let mut failed = Vec::new();
        for tab in self.tabs.iter_mut().filter(|t| t.dirty()) {
            let path = tab.path.clone();
            if let Ok(doc) = tab.doc.as_mut() {
                if let Err(why) = doc.save(&path) {
                    failed.push(format!("{}: {why}", tab.name));
                }
            }
        }
        failed
    }

    /// A file opened, first among those opened before, and kept.
    fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT);
        self.keep();
    }

    /// The files opened before, and where each PDF was left, kept.
    fn keep(&self) {
        let Some(file) = &self.store else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut read: Vec<(PathBuf, u32, u64)> = self.read.iter().map(|(p, (page, at))| (p.clone(), *page, *at)).collect();
        // The most recent thousand: a reader's whole shelf, and no more.
        read.sort_by(|a, b| b.2.cmp(&a.2));
        read.truncate(1000);
        let kept = Kept { recent: self.recent.clone(), read };
        let _ = proton_research::store::atomic_write(file, serde_json::to_string_pretty(&kept).unwrap_or_default().as_bytes());
    }
}

/// Seconds since the epoch.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A tab drawn: at `page`, or where the window has it.
fn view_event(tab: &Tab, page: Option<u32>, quiet: bool) -> Event {
    Event::PdfView { tab: tab.id, key: tab.key(), version: tab.version, page, quiet }
}

/// A tab's comments, as it is now — and its form's fields.
fn comments_event(tab: &Tab) -> Event {
    Event::PdfComments { tab: tab.id, comments: tab.doc.as_ref().map(|d| d.comments()).unwrap_or_default() }
}

fn fields_event(tab: &Tab) -> Event {
    Event::PdfFields { tab: tab.id, fields: tab.doc.as_ref().map(|d| d.fields()).unwrap_or_default() }
}

/// What a tab says of itself, and how large it is.
fn info_event(tab: &Tab) -> Event {
    let doc = tab.doc.as_ref().ok();
    Event::PdfInfo {
        tab: tab.id,
        info: doc.map(|d| d.info()).unwrap_or_default(),
        protected: doc.is_some_and(|d| d.protected()),
        on_disk: doc.map(|d| d.size_on_disk()).unwrap_or(tab.view.len()) as u32,
        now: doc.filter(|d| d.dirty()).and_then(|d| d.bytes().ok()).map(|b| b.len()).unwrap_or_else(|| doc.map(|d| d.size_on_disk()).unwrap_or(tab.view.len())) as u32,
    }
}

/// A file's bytes, if it is a PDF and not too large to open.
fn read(path: &Path) -> Result<Vec<u8>, String> {
    let shown = folder::tilde(path);
    let meta = std::fs::metadata(path).map_err(|e| format!("{shown}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{shown}: not a file"));
    }
    if meta.len() > LARGEST {
        return Err(format!("{shown}: {} MB is more than a PDF is opened at", meta.len() / (1024 * 1024)));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    // `%PDF-` near the start: some writers put a few bytes before it.
    if !bytes.windows(5).take(1024).any(|w| w == b"%PDF-") {
        return Err(format!("{shown}: not a PDF"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_librarys_folders_are_the_persons_own_setting_written_when_changed() {
        let dir = std::env::temp_dir().join(format!("proton-pdf-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Books")).unwrap();
        let file = dir.join("settings.json");
        std::fs::write(&file, serde_json::json!({"pdf": {"library": [dir.join("Books")]}, "other": 1}).to_string()).unwrap();
        let mut pdfs = Pdfs::new().with_person_settings(&file);
        assert_eq!(pdfs.library.folders(), [dir.join("Books").display().to_string()]);
        let events = pdfs.set_library(vec![format!("{}/", dir.join("Books").display()), "  ".into(), dir.join("Gone").display().to_string()]);
        assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.ends_with("Gone: no such folder"))), "{events:?}");
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(written["pdf"]["library"], serde_json::json!([dir.join("Books").display().to_string(), dir.join("Gone").display().to_string()]));
        assert_eq!(written["other"], 1, "the rest of the person's settings kept");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_is_kept_is_read_back_whether_written_as_a_list_once_or_with_where_each_was_left() {
        let dir = std::env::temp_dir().join(format!("proton-pdf-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let book = dir.join("book.pdf");
        std::fs::write(&book, b"%PDF").unwrap();
        let file = dir.join("pdf.json");
        std::fs::write(&file, serde_json::to_string(&vec![&book]).unwrap()).unwrap();
        assert_eq!(Pdfs::kept_in(file.clone()).recent, [book.clone()], "the list it once was");
        std::fs::write(&file, serde_json::json!({"recent": [&book], "read": [[&book, 42, 1_790_000_000u64]]}).to_string()).unwrap();
        let kept = Pdfs::kept_in(file);
        assert_eq!((kept.recent.clone(), kept.read.get(&book).copied()), (vec![book.clone()], Some((42, 1_790_000_000))));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_view_key_names_one_version_of_one_open_tab() {
        let mut pdfs = Pdfs::new();
        pdfs.tabs.push(Tab { id: 3, path: PathBuf::from("/x.pdf"), name: "x.pdf".into(), view: Arc::new(b"%PDF-1.7".to_vec()), version: 2, doc: Err("a test".into()), at: 1, read: Default::default() });
        assert_eq!(pdfs.view_bytes("pdfview:3:2").as_deref().map(Vec::as_slice), Some(&b"%PDF-1.7"[..]));
        for stale in ["pdfview:3:1", "pdfview:4:2", "pdfview:3", "/x.pdf", "pdfview:a:b"] {
            assert!(pdfs.view_bytes(stale).is_none(), "{stale}");
        }
    }
}
