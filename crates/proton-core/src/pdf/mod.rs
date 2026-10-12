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
    matches!(command, Command::PdfOpen { .. } | Command::PdfClose { .. } | Command::PdfFocus { .. })
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
}

impl Tab {
    fn key(&self) -> String {
        format!("{VIEW}{}:{}", self.id, self.version)
    }

    fn view(&self) -> TabView {
        TabView { id: self.id, name: self.name.clone(), path: folder::tilde(&self.path), dirty: false }
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
            Command::PdfClose { tab } => self.close(tab),
            Command::PdfFocus { tab } => self.focus(tab),
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
        let tab = Tab { id: self.next, name: folder::name_of(&path), path: path.clone(), view: Arc::new(bytes), version: 1 };
        self.active = Some(tab.id);
        let view = view_event(&tab, page, false);
        self.tabs.push(tab);
        self.remember(&path);
        vec![self.tabs_event(), view]
    }

    fn close(&mut self, id: u32) -> Vec<Event> {
        let Some(at) = self.tabs.iter().position(|t| t.id == id) else { return Vec::new() };
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
        pdfs.tabs.push(Tab { id: 3, path: PathBuf::from("/x.pdf"), name: "x.pdf".into(), view: Arc::new(b"%PDF-1.7".to_vec()), version: 2 });
        assert_eq!(pdfs.view_bytes("pdfview:3:2").as_deref().map(Vec::as_slice), Some(&b"%PDF-1.7"[..]));
        for stale in ["pdfview:3:1", "pdfview:4:2", "pdfview:3", "/x.pdf", "pdfview:a:b"] {
            assert!(pdfs.view_bytes(stale).is_none(), "{stale}");
        }
    }
}
