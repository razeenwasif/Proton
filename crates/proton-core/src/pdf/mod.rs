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
}

impl Pdfs {
    /// Nothing open, nothing kept: for tests.
    pub fn new() -> Pdfs {
        Pdfs::default()
    }

    /// The files opened before, kept in `file`.
    pub fn kept_in(file: PathBuf) -> Pdfs {
        let recent = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<PathBuf>>(&text).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.is_file())
            .collect();
        Pdfs { recent, store: Some(file), ..Pdfs::default() }
    }

    /// The mode entered, over the project at `root`, whose PDFs are `pdfs`:
    /// the tabs, and the one on top drawn.
    pub fn enter(&mut self, root: &Path, pdfs: Vec<String>) -> Vec<Event> {
        self.project = (pdfs, root.to_path_buf());
        let mut events = vec![self.tabs_event()];
        if let Some(tab) = self.active_tab() {
            events.push(view_event(tab, None, false));
        }
        events
    }

    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::PdfOpen { path, page } => self.open(&path, page),
            Command::PdfClose { tab, discard } => self.close(tab, discard),
            Command::PdfFocus { tab } => self.focus(tab),
            Command::PdfUndo => self.history(false),
            Command::PdfRedo => self.history(true),
            Command::PdfSave => self.save(None),
            Command::PdfSaveAs { path } => self.save(Some(&path)),
            Command::PdfRotate { pages, by } => self.change(proton_pdf::Op::Rotate { pages, by }),
            Command::PdfDelete { pages } => self.change(proton_pdf::Op::Delete { pages }),
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
        let typed = typed.trim();
        if typed.is_empty() {
            return vec![notice("Say which PDF to open.".into(), NoticeLevel::Info)];
        }
        let expanded = folder::expand(typed);
        let path = if expanded.is_absolute() { expanded } else { self.project.1.join(expanded) };
        let path = folder::normalise(&path);
        if let Some(tab) = self.tabs.iter().find(|t| t.path == path) {
            self.active = Some(tab.id);
            return vec![self.tabs_event(), view_event(tab, page, false)];
        }
        let bytes = match read(&path) {
            Ok(bytes) => bytes,
            Err(why) => return vec![notice(why, NoticeLevel::Error)],
        };
        self.next += 1;
        let view = Arc::new(bytes);
        let doc = match proton_pdf::Doc::open(view.clone()) {
            proton_pdf::Opened::Editable(doc) => Ok(doc),
            proton_pdf::Opened::ReadOnly(why) => Err(why),
        };
        let tab = Tab { id: self.next, name: folder::name_of(&path), path: path.clone(), view, version: 1, doc };
        self.active = Some(tab.id);
        let view = view_event(&tab, page, false);
        self.tabs.push(tab);
        self.remember(&path);
        vec![self.tabs_event(), view]
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
        }
        events
    }

    fn focus(&mut self, id: u32) -> Vec<Event> {
        let Some(tab) = self.tabs.iter().find(|t| t.id == id) else { return Vec::new() };
        self.active = Some(id);
        vec![self.tabs_event(), view_event(tab, None, false)]
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
        let Some(id) = self.active else { return Vec::new() };
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else { return Vec::new() };
        let bytes = match tab.doc.as_ref().map(|d| d.bytes()) {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(why)) => return vec![notice(why, NoticeLevel::Error)],
            Err(_) => return Vec::new(),
        };
        tab.view = Arc::new(bytes);
        tab.version += 1;
        let view = view_event(tab, None, false);
        vec![self.tabs_event(), view]
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
        if let Some(file) = &self.store {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(file, serde_json::to_string_pretty(&self.recent).unwrap_or_default());
        }
    }
}

/// A tab drawn: at `page`, or where the window has it.
fn view_event(tab: &Tab, page: Option<u32>, quiet: bool) -> Event {
    Event::PdfView { tab: tab.id, key: tab.key(), version: tab.version, page, quiet }
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
    fn a_view_key_names_one_version_of_one_open_tab() {
        let mut pdfs = Pdfs::new();
        pdfs.tabs.push(Tab { id: 3, path: PathBuf::from("/x.pdf"), name: "x.pdf".into(), view: Arc::new(b"%PDF-1.7".to_vec()), version: 2, doc: Err("a test".into()) });
        assert_eq!(pdfs.view_bytes("pdfview:3:2").as_deref().map(Vec::as_slice), Some(&b"%PDF-1.7"[..]));
        for stale in ["pdfview:3:1", "pdfview:4:2", "pdfview:3", "/x.pdf", "pdfview:a:b"] {
            assert!(pdfs.view_bytes(stale).is_none(), "{stale}");
        }
    }
}
