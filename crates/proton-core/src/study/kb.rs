//! The knowledge base a notebook's sources are chosen from: the folders the
//! person names, less what their rules leave out, catalogued and read by a
//! shelf in catalog mode (`proton_library::shelf`).
//!
//! Nothing is walked until Notebook is first opened, so a person who never
//! uses it pays nothing for it. Then the catalogue comes in a second, the
//! notes in a second more, and the PDFs over minutes the first time, while
//! the picker already works.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proton_library::catalog::{Catalog, FlagFn, Root};
use proton_library::{pdf, Rules, Shelf, Why};
use proton_study::view::{StudyKbFile, StudyKbFolder, StudyKbHit, StudyKbLeft, StudyRoot};

use crate::bus::Event;

/// What the knowledge base last said, so it is said again only when it moved.
#[derive(Debug, Clone, PartialEq)]
struct Said {
    walking: bool,
    progress: (usize, usize),
    paused: bool,
    documents: usize,
    ready: bool,
}

/// The knowledge base.
pub struct Kb {
    /// The roots as typed — `~/Courses` — and as found.
    typed: Vec<String>,
    roots: Vec<PathBuf>,
    rules: Rules,
    pdfs: bool,
    pdftotext: bool,
    shelf: Shelf,
    /// Whether the shelf has been given its roots: on first entering Notebook.
    started: bool,
    said: Option<Said>,
}

impl Default for Kb {
    fn default() -> Kb {
        Kb { typed: Vec::new(), roots: Vec::new(), rules: Rules::default(), pdfs: true, pdftotext: false, shelf: Shelf::new(), started: false, said: None }
    }
}

impl Kb {
    /// The roots, the rules, and how PDFs are read. Applied at once if the
    /// knowledge base has been started, and kept for when it is otherwise.
    pub fn configure(&mut self, typed: Vec<String>, exclude: Vec<String>, pdfs: bool, pdftotext: bool) {
        self.roots = typed.iter().filter_map(|t| crate::settings::research_path(t)).collect();
        self.typed = typed;
        self.rules = Rules::new(exclude);
        self.pdfs = pdfs;
        self.pdftotext = pdftotext;
        if self.started {
            self.apply();
        }
    }

    /// Say which files are in iCloud with this rather than their flags: a test's.
    pub fn flags_with(&mut self, flags: FlagFn) {
        self.shelf.flags_with(flags);
    }

    /// Begin: catalogue the roots and read them.
    pub fn start(&mut self) {
        if !self.started {
            self.started = true;
            self.shelf.cache_in(pdf::cache_dir());
            self.apply();
        }
    }

    fn apply(&mut self) {
        let extractor = match (self.pdftotext, pdf::find_pdftotext()) {
            (true, Some(exe)) => pdf::Extractor::Pdftotext(exe),
            _ => pdf::Extractor::PdfExtract,
        };
        self.shelf.extract_with(extractor);
        self.shelf.set_pdfs(self.pdfs);
        self.shelf.read_catalog(Some(self.rules.clone()));
        self.shelf.set_roots(self.roots.clone());
    }

    pub fn typed(&self) -> &[String] {
        &self.typed
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    pub fn catalog(&self) -> Option<&Catalog> {
        self.shelf.catalog()
    }

    pub fn pause(&mut self, paused: bool) {
        self.shelf.pause(paused);
    }

    /// Look again: the catalogue and what changed, keeping what is here
    /// meanwhile.
    pub fn refresh(&mut self) {
        if self.started {
            self.shelf.refresh();
        }
    }

    /// Take what the shelf has finished; `true` when there is something new
    /// to say.
    pub fn poll(&mut self) -> bool {
        if !self.started {
            return false;
        }
        self.shelf.poll();
        let now = self.said_now();
        if self.said.as_ref() == Some(&now) {
            return false;
        }
        self.said = Some(now);
        true
    }

    fn said_now(&self) -> Said {
        Said {
            walking: self.shelf.is_building(),
            progress: self.shelf.progress(),
            paused: self.shelf.is_paused(),
            documents: self.shelf.documents(),
            ready: self.shelf.catalog().is_some(),
        }
    }

    /// Where a file of the knowledge base is, by its labelled path — only if
    /// the catalogue lists it, so a path left out by a rule, hidden, or made up
    /// is nowhere.
    pub fn locate(&self, labelled: &str) -> Option<(PathBuf, &proton_library::catalog::Entry)> {
        let catalog = self.catalog()?;
        let entry = catalog.get(labelled)?;
        Some((catalog.path(entry), entry))
    }

    /// Every root, labelled as citations and rules name it.
    pub fn labelled_roots(&self) -> Vec<(String, PathBuf)> {
        Root::labelled(self.roots.clone()).into_iter().map(|r| (r.label, r.path)).collect()
    }

    /// The root a label names, on the disk.
    pub fn root_path(&self, label: &str) -> Option<PathBuf> {
        Root::labelled(self.roots.clone()).into_iter().find(|r| r.label == label).map(|r| r.path)
    }

    /// What the window shows of it.
    pub fn event(&self) -> Event {
        let labelled = Root::labelled(self.roots.clone());
        let catalog = self.catalog();
        let roots = labelled
            .iter()
            .enumerate()
            .map(|(i, root)| {
                let typed = self.typed.get(i).cloned().unwrap_or_else(|| crate::folder::tilde(&root.path));
                let (mut files, mut readable, mut in_cloud) = (0, 0, 0);
                let mut missing = !root.path.is_dir();
                if let Some(catalog) = catalog {
                    if let Some(at) = catalog.root_named(&root.label) {
                        for entry in catalog.entries().iter().filter(|e| e.root == at) {
                            files += 1;
                            readable += usize::from(entry.kind.indexed());
                            in_cloud += usize::from(entry.dataless);
                        }
                        missing = catalog.missing().iter().any(|m| m.label == root.label);
                    }
                }
                StudyRoot { label: root.label.clone(), path: typed, files, readable, in_cloud, missing }
            })
            .collect();
        let (done, total) = self.shelf.progress();
        Event::StudyKb {
            roots,
            rules: self.rules.written().to_vec(),
            started: self.started,
            ready: catalog.is_some(),
            walking: self.shelf.is_building(),
            done,
            total,
            paused: self.shelf.is_paused(),
            documents: self.shelf.documents(),
            passages: self.shelf.len(),
        }
    }

    /// One folder of a root: `dir` empty for the root itself. `sources` says
    /// which files are already sources of the open notebook.
    pub fn folder(&self, root: &str, dir: &str, sources: &HashMap<String, String>) -> Event {
        let dir = dir.trim_matches('/').to_string();
        let mut out = Event::StudyKbFolder { root: root.to_string(), dir: dir.clone(), folders: Vec::new(), files: Vec::new(), left: Vec::new() };
        let Some(catalog) = self.catalog() else { return out };
        let Some(at) = catalog.root_named(root) else { return out };
        let listing = catalog.list(at, &dir);
        let under = |name: &str| match dir.as_str() {
            "" => format!("{root}/{name}"),
            dir => format!("{root}/{dir}/{name}"),
        };
        if let Event::StudyKbFolder { folders, files, left, .. } = &mut out {
            *folders = listing.folders.iter().map(|f| StudyKbFolder { name: f.name.clone(), path: under(&f.name), files: f.files, readable: f.readable, in_cloud: f.dataless }).collect();
            *files = listing.files.iter().map(|e| StudyKbFile::of(catalog, e, sources.get(&catalog.labelled(e)).cloned())).collect();
            *left = listing
                .skipped
                .iter()
                .map(|s| {
                    let name = s.rel.rsplit('/').next().unwrap_or(&s.rel).to_string();
                    let rule = match &s.why {
                        Why::Yours(rule) => Some(rule.clone()),
                        _ => None,
                    };
                    StudyKbLeft { path: under(&name), name, dir: s.dir, why: s.why.say(), rule }
                })
                .collect();
        }
        out
    }

    /// A search: files by the words of their names, and passages by the
    /// words in them.
    pub fn search(&self, query: &str, sources: &HashMap<String, String>) -> Event {
        let query = query.trim();
        let files = match self.catalog() {
            Some(catalog) if !query.is_empty() => catalog.find(query, 40).into_iter().map(|e| StudyKbFile::of(catalog, e, sources.get(&catalog.labelled(e)).cloned())).collect(),
            _ => Vec::new(),
        };
        let passages = match query.is_empty() {
            true => Vec::new(),
            false => self.shelf.search(query, 20).iter().map(StudyKbHit::of).collect(),
        };
        Event::StudyKbResults { query: query.to_string(), files, passages }
    }

    /// Where a root's files are on the disk: for a source's file, by its
    /// root's label and its path under it.
    pub fn file(&self, root: &str, rel: &str) -> Option<PathBuf> {
        let base = self.root_path(root)?;
        let rel = Path::new(rel);
        if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return None;
        }
        Some(base.join(rel))
    }
}
