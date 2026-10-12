//! The PDF mode's library: the books in the folders the person keeps them in
//! — `~/Documents/Books` until they name others — shelved by the folder each
//! is in, each with its title and author as the book says them, how far it
//! was read, and its cover.
//!
//! Walked and read on a worker, and kept beside the session, so the next
//! library is on screen at once while it is walked again. A cover is drawn
//! only once the window shows its book, by Poppler in the sandbox, and kept
//! too. A book whose file is in iCloud is listed and never read, so nothing
//! is brought down until the person opens it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::Event;
use crate::folder;

/// The most books one library lists.
const MOST: usize = 5_000;
/// The most files and folders its walk looks at.
const WALKED: usize = 50_000;
/// Folders that hold other people's code, not books.
const PASSED: [&str; 6] = ["node_modules", "target", "build", "dist", "__pycache__", "venv"];

/// A book, as the library shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "PdfBook")]
#[serde(rename_all = "camelCase")]
pub struct Book {
    /// As the person would write it: `~/Documents/Books/ML/mml-book.pdf`.
    pub path: String,
    pub file: String,
    pub title: String,
    pub author: String,
    pub pages: u32,
    #[ts(type = "number")]
    pub size: u64,
    /// The folder it is in under the library's — its shelf; empty for the
    /// library folder's own.
    pub shelf: String,
    /// Which of the library's folders it is in: `~/Documents/Books`.
    pub folder: String,
    /// Its file is in iCloud: opening it brings it down.
    pub in_cloud: bool,
    /// The page it was left at.
    pub page: Option<u32>,
    /// When it was last read, in seconds since the epoch.
    #[ts(type = "number | null")]
    pub read: Option<u64>,
    /// Its title or author is the person's, not the book's own.
    pub renamed: bool,
}

/// A book's cover, drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "PdfCover")]
pub struct Cover {
    pub path: String,
    /// `data:image/jpeg;base64,…`; none when it cannot be drawn here.
    pub data: Option<String>,
}

/// A book as walked and read: kept between sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Shelved {
    path: PathBuf,
    folder: PathBuf,
    shelf: String,
    size: u64,
    mtime: u64,
    in_cloud: bool,
    title: String,
    author: String,
    pages: u32,
}

/// Where a book was left, and when.
pub type Read = HashMap<PathBuf, (u32, u64)>;

/// What the person calls a book, in place of what it calls itself: kept
/// apart from what the walk reads, so reading it again changes neither. An
/// empty one is the book's own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Named {
    title: String,
    author: String,
}

/// `pdf-library.json`: the books as walked, and what the person calls them.
/// Once only the books, as a list.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Kept {
    #[serde(default)]
    books: Vec<Shelved>,
    #[serde(default)]
    names: Vec<(PathBuf, Named)>,
}

/// The library.
#[derive(Default)]
pub struct Library {
    /// The folders as typed — `~/Documents/Books`.
    typed: Vec<String>,
    /// As last walked.
    shelved: Vec<Shelved>,
    walking: Option<Receiver<Vec<Shelved>>>,
    /// Walked since the session began.
    walked: bool,
    /// `pdf-library.json`, and the covers' folder; none in tests.
    store: Option<PathBuf>,
    covers_dir: Option<PathBuf>,
    /// Poppler's `pdftoppm`, or a stand-in.
    tool: Option<PathBuf>,
    drawer: Option<(Sender<(PathBuf, u64, u64)>, Receiver<(PathBuf, Option<Vec<u8>>)>)>,
    drawing: HashSet<PathBuf>,
    /// Covers this session has, as the window takes them.
    drawn: HashMap<PathBuf, Option<String>>,
    /// What the person calls books, by path.
    names: HashMap<PathBuf, Named>,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library").field("typed", &self.typed).field("books", &self.shelved.len()).field("walking", &self.walking.is_some()).finish()
    }
}

impl Library {
    /// Kept in `dir`: the books as last walked, and their covers.
    pub fn kept_in(dir: &Path) -> Library {
        let store = dir.join("pdf-library.json");
        let text = std::fs::read_to_string(&store).unwrap_or_default();
        let kept = serde_json::from_str::<Kept>(&text)
            .or_else(|_| serde_json::from_str::<Vec<Shelved>>(&text).map(|books| Kept { books, names: Vec::new() }))
            .unwrap_or_default();
        Library { shelved: kept.books, names: kept.names.into_iter().collect(), store: Some(store), covers_dir: Some(dir.join("pdf-covers")), ..Library::default() }
    }

    /// Covers drawn by this program: Poppler's, or a test's stand-in.
    pub fn drawn_by(&mut self, tool: Option<PathBuf>) {
        self.tool = tool;
    }

    /// The folders, as typed. Kept books from folders no longer named go.
    pub fn set_folders(&mut self, typed: Vec<String>) {
        let roots: Vec<PathBuf> = typed.iter().map(|t| resolved(t)).collect();
        self.shelved.retain(|s| roots.contains(&s.folder));
        self.typed = typed;
    }

    pub fn folders(&self) -> &[String] {
        &self.typed
    }

    /// Whether the session has walked it yet.
    pub fn walked(&self) -> bool {
        self.walked
    }

    /// Walk the folders again, on a worker: what is kept of a book whose
    /// file is unchanged is not read again.
    pub fn walk(&mut self) {
        self.walked = true;
        let roots: Vec<PathBuf> = self.typed.iter().map(|t| resolved(t)).collect();
        let known: HashMap<PathBuf, Shelved> = self.shelved.iter().map(|s| (s.path.clone(), s.clone())).collect();
        let (send, receive) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("proton-pdf-library".into()).spawn(move || {
            let _ = send.send(shelve(&roots, &known));
        });
        if spawned.is_ok() {
            self.walking = Some(receive);
        }
    }

    /// The library, as the window draws it: each book where it was left.
    pub fn event(&self, read: &Read) -> Event {
        let missing = self.typed.iter().filter(|t| !folder::expand(t).is_dir()).cloned().collect();
        let books = self
            .shelved
            .iter()
            .map(|s| {
                let left = read.get(&s.path);
                let named = self.names.get(&s.path);
                let title = named.map(|n| n.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| s.title.clone());
                let author = named.map(|n| n.author.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| s.author.clone());
                Book {
                    path: folder::tilde(&s.path),
                    file: folder::name_of(&s.path),
                    title,
                    author,
                    pages: s.pages,
                    size: s.size,
                    shelf: s.shelf.clone(),
                    folder: folder::tilde(&s.folder),
                    in_cloud: s.in_cloud,
                    page: left.map(|l| l.0),
                    read: left.map(|l| l.1),
                    renamed: named.is_some(),
                }
            })
            .collect();
        Event::PdfLibrary { folders: self.typed.clone(), books, walking: self.walking.is_some(), missing }
    }

    /// What has come of the walk and of covers being drawn: whether the
    /// library changed, and the covers drawn since.
    pub fn poll(&mut self) -> (bool, Vec<Cover>) {
        let mut changed = false;
        if let Some(walking) = &self.walking {
            match walking.try_recv() {
                Ok(shelved) => {
                    self.shelved = shelved;
                    self.walking = None;
                    self.keep();
                    changed = true;
                }
                Err(TryRecvError::Disconnected) => {
                    self.walking = None;
                    changed = true;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut covers = Vec::new();
        if let Some((_, answers)) = &self.drawer {
            while let Ok((path, bytes)) = answers.try_recv() {
                self.drawing.remove(&path);
                let data = bytes.map(|b| data_uri(&b));
                self.drawn.insert(path.clone(), data.clone());
                covers.push(Cover { path: folder::tilde(&path), data });
            }
        }
        (changed, covers)
    }

    /// Covers for these books: those drawn already, now; the rest drawn on a
    /// worker and given by [`poll`](Self::poll). Only a book of the library
    /// is drawn — never another file a window names.
    pub fn covers(&mut self, paths: &[String]) -> Vec<Cover> {
        let mut now = Vec::new();
        for typed in paths {
            let path = resolved(typed);
            let Some(book) = self.shelved.iter().find(|s| s.path == path).cloned() else {
                now.push(Cover { path: typed.clone(), data: None });
                continue;
            };
            if let Some(data) = self.drawn.get(&path) {
                now.push(Cover { path: typed.clone(), data: data.clone() });
                continue;
            }
            if let Some(bytes) = self.cached(&book).and_then(|f| std::fs::read(f).ok()) {
                let data = Some(data_uri(&bytes));
                self.drawn.insert(path, data.clone());
                now.push(Cover { path: typed.clone(), data });
                continue;
            }
            // Nothing to draw it with, or it would be brought down from iCloud.
            if book.in_cloud || self.tool.is_none() {
                now.push(Cover { path: typed.clone(), data: None });
                continue;
            }
            if self.drawing.insert(path.clone()) {
                if let Some((send, _)) = self.drawer() {
                    let _ = send.send((path, book.size, book.mtime));
                }
            }
        }
        now
    }

    /// Where a book's cover is kept, for its file as it is.
    fn cached(&self, book: &Shelved) -> Option<PathBuf> {
        Some(cover_file(self.covers_dir.as_ref()?, &book.path, book.size, book.mtime))
    }

    /// The worker that draws covers, started when the first is asked for.
    fn drawer(&mut self) -> Option<&(Sender<(PathBuf, u64, u64)>, Receiver<(PathBuf, Option<Vec<u8>>)>)> {
        if self.drawer.is_none() {
            let tool = self.tool.clone()?;
            let covers = self.covers_dir.clone();
            let (ask, asked) = mpsc::channel::<(PathBuf, u64, u64)>();
            let (answer, answers) = mpsc::channel();
            let kind = proton_lab::sandbox::available();
            std::thread::Builder::new()
                .name("proton-pdf-covers".into())
                .spawn(move || {
                    let work = covers.clone().unwrap_or_else(std::env::temp_dir);
                    let _ = std::fs::create_dir_all(&work);
                    for (path, size, mtime) in asked {
                        let drawn = proton_lab::cover::draw(&tool, &path, &work, kind).ok();
                        if let (Some(bytes), Some(dir)) = (&drawn, &covers) {
                            let _ = proton_research::store::atomic_write(&cover_file(dir, &path, size, mtime), bytes);
                        }
                        if answer.send((path, drawn)).is_err() {
                            return;
                        }
                    }
                })
                .ok()?;
            self.drawer = Some((ask, answers));
        }
        self.drawer.as_ref()
    }

    /// Call a book of the library by another title and author, in the
    /// library only: its file and what it says of itself are left as they
    /// are. Both empty, it is called by its own again.
    pub fn rename(&mut self, typed: &str, title: &str, author: &str) -> Result<(), String> {
        let path = resolved(typed);
        let book = self.shelved.iter().find(|s| s.path == path).ok_or_else(|| format!("{typed} is not a book of the library"))?;
        let (title, author) = (title.trim(), author.trim());
        if title.chars().count() > 300 || author.chars().count() > 300 {
            return Err("A title or an author is three hundred letters at most.".into());
        }
        // What it calls itself already is no renaming.
        let title = if title == book.title { "" } else { title };
        let author = if author == book.author { "" } else { author };
        match title.is_empty() && author.is_empty() {
            true => self.names.remove(&path),
            false => self.names.insert(path, Named { title: title.to_string(), author: author.to_string() }),
        };
        self.keep();
        Ok(())
    }

    /// The books as walked, and what the person calls them, kept for the
    /// next session.
    fn keep(&self) {
        if let Some(file) = &self.store {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let mut names: Vec<(PathBuf, Named)> = self.names.iter().map(|(p, n)| (p.clone(), n.clone())).collect();
            names.sort_by(|a, b| a.0.cmp(&b.0));
            let kept = Kept { books: self.shelved.clone(), names };
            let _ = proton_research::store::atomic_write(file, serde_json::to_string(&kept).unwrap_or_default().as_bytes());
        }
    }
}

/// A cover's file, named for its book's file as it is: a book changed is
/// drawn again.
fn cover_file(dir: &Path, path: &Path, size: u64, mtime: u64) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (path, size, mtime).hash(&mut hash);
    dir.join(format!("{:016x}.jpg", hash.finish()))
}

/// A path as typed, `~` and all, resolved as an opened PDF's is — so a
/// book on a shelf and the same book open in a tab are one.
fn resolved(typed: &str) -> PathBuf {
    folder::normalise(&folder::expand(typed))
}

fn data_uri(bytes: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Every PDF in the folders, shelved by the folder it is in, each read for
/// its title, author and pages unless `known` has it as its file still is.
fn shelve(roots: &[PathBuf], known: &HashMap<PathBuf, Shelved>) -> Vec<Shelved> {
    let mut out = Vec::new();
    let mut walked = 0;
    for root in roots {
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                walked += 1;
                if walked > WALKED || out.len() >= MOST {
                    return out;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    // A folder whose listing is in iCloud is not opened: that
                    // would bring it down.
                    if !PASSED.contains(&name.as_str()) && !proton_library::dataless::is_dataless(&meta) {
                        pending.push(entry.path());
                    }
                    continue;
                }
                let path = entry.path();
                if !meta.is_file() || !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
                    continue;
                }
                let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                let in_cloud = proton_library::dataless::is_dataless(&meta);
                let shelf = path.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                let found = Shelved { path: path.clone(), folder: root.clone(), shelf, size: meta.len(), mtime, in_cloud, title: String::new(), author: String::new(), pages: 0 };
                let read = match known.get(&path) {
                    Some(k) if (k.size, k.mtime, k.in_cloud) == (found.size, found.mtime, found.in_cloud) && !k.title.is_empty() => Shelved { title: k.title.clone(), author: k.author.clone(), pages: k.pages, ..found },
                    // Its file is in iCloud: what its name says, and no more.
                    _ if in_cloud => Shelved { title: proton_pdf::library::from_name(&path.file_stem().unwrap_or_default().to_string_lossy()).0, ..found },
                    _ => {
                        let about = proton_pdf::library::about(&path);
                        Shelved { title: about.title, author: about.author, pages: about.pages, ..found }
                    }
                };
                out.push(read);
            }
        }
    }
    out.sort_by(|a, b| (a.shelf.to_lowercase(), a.title.to_lowercase()).cmp(&(b.shelf.to_lowercase(), b.title.to_lowercase())));
    out
}
