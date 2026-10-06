//! Every file of a knowledge base, listed without opening one.
//!
//! The picker a notebook's sources are chosen from needs the whole tree of
//! several roots at once — names, kinds, sizes, and which files are still in
//! iCloud — and it needs it in a second or two, not after minutes of reading
//! PDFs. So this is a walk that only ever asks the disk what is there: one
//! `lstat` per entry, never an `open`. Measured against the roots it was
//! written for, 71,656 files under three roots stat in about 1.7 seconds.
//!
//! What it leaves out it says, with the reason ([`Why`]): a vendor folder, a
//! rule of the person's, a dataset, a folder still in iCloud. What it walks
//! past without saying: hidden files and folders (`.git`, `.obsidian`), and
//! symbolic links, which are never followed — a link inside a root pointing at
//! `~/Documents` would make the root a lie.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::dataless;
use crate::kind::Kind;
use crate::rules::{RootRules, Rules, Why};

/// A root of the knowledge base, and the name citations and rules call it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub label: String,
    pub path: PathBuf,
}

impl Root {
    pub fn new(label: impl Into<String>, path: impl Into<PathBuf>) -> Root {
        Root { label: label.into(), path: path.into() }
    }

    /// Roots named by their last folder — `OnyxVault`, `Courses`, `ANU` — with
    /// a number added where two would share a name.
    pub fn labelled(paths: impl IntoIterator<Item = PathBuf>) -> Vec<Root> {
        let mut out: Vec<Root> = Vec::new();
        for path in paths {
            let base = path
                .file_name()
                .map(|n| n.to_string_lossy().replace('/', "-"))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "Root".to_string());
            let mut label = base.clone();
            let mut n = 2;
            while out.iter().any(|r| r.label.eq_ignore_ascii_case(&label)) {
                label = format!("{base} {n}");
                n += 1;
            }
            out.push(Root { label, path });
        }
        out
    }
}

/// One file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Which root, as an index into [`Catalog::roots`].
    pub root: usize,
    /// Relative to the root, with `/` between folders.
    pub rel: String,
    pub kind: Kind,
    pub size: u64,
    /// Seconds since the epoch.
    pub mtime: u64,
    /// Its contents are in iCloud: reading it would download it.
    pub dataless: bool,
}

/// A folder or file left out, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub root: usize,
    /// Relative to the root; empty for the whole root.
    pub rel: String,
    pub dir: bool,
    pub why: Why,
}

/// A folder's direct contents, for a picker that opens one folder at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing<'a> {
    pub folders: Vec<Folder>,
    pub files: Vec<&'a Entry>,
    pub skipped: Vec<&'a Skipped>,
}

/// A folder inside the one listed, with what is under it at any depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub files: usize,
    /// Files whose words can be searched.
    pub readable: usize,
    /// Files still in iCloud.
    pub dataless: usize,
}

/// What a walk found under every root.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    roots: Vec<Root>,
    /// Sorted by root, then by path.
    entries: Vec<Entry>,
    skipped: Vec<Skipped>,
    /// Roots named that are not directories.
    missing: Vec<usize>,
    elapsed: Duration,
}

/// What a walk asks of each entry's metadata: its BSD flags. Real walks read
/// `st_flags`; a test says which of its files are in iCloud.
pub type Flags<'a> = &'a dyn Fn(&Path, &std::fs::Metadata) -> u32;

impl Catalog {
    /// Walk every root.
    pub fn walk(roots: &[Root], rules: &Rules) -> Catalog {
        Catalog::walk_with(roots, rules, &|_, meta| dataless::flags(meta))
    }

    /// Walk every root, asking `flags` which entries are in iCloud.
    pub fn walk_with(roots: &[Root], rules: &Rules, flags: Flags) -> Catalog {
        let started = Instant::now();
        // Nothing here opens a file; if something ever did, it fails rather
        // than downloading.
        let _guard = dataless::NoDownloads::on_this_thread();
        let mut out = Catalog { roots: roots.to_vec(), ..Catalog::default() };
        for (index, root) in roots.iter().enumerate() {
            // The same folder named twice is listed once.
            if roots[..index].iter().any(|earlier| earlier.path == root.path) {
                continue;
            }
            let under = rules.under(&root.label);
            if let Some(why) = under.whole() {
                out.skipped.push(Skipped { root: index, rel: String::new(), dir: true, why });
                continue;
            }
            if !root.path.is_dir() {
                out.missing.push(index);
                continue;
            }
            // A root inside this one — a courses folder and one course in it —
            // is listed under its own label, not twice.
            let inner: Vec<PathBuf> = roots.iter().filter(|other| other.path != root.path && other.path.starts_with(&root.path)).map(|other| other.path.clone()).collect();
            let mut walker = Walker {
                root: index,
                inner: &inner,
                rules: &under,
                dataset: rules.dataset(),
                flags,
                entries: &mut out.entries,
                skipped: &mut out.skipped,
            };
            walker.visit(&root.path, "");
        }
        out.entries.sort_by(|a, b| (a.root, &a.rel).cmp(&(b.root, &b.rel)));
        out.skipped.sort_by(|a, b| (a.root, &a.rel).cmp(&(b.root, &b.rel)));
        out.elapsed = started.elapsed();
        out
    }

    pub fn roots(&self) -> &[Root] {
        &self.roots
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }

    /// Roots named that are not directories: worth saying, since a typo
    /// otherwise reads as an empty root.
    pub fn missing(&self) -> Vec<&Root> {
        self.missing.iter().map(|i| &self.roots[*i]).collect()
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Where an entry is on the disk.
    pub fn path(&self, entry: &Entry) -> PathBuf {
        self.roots[entry.root].path.join(&entry.rel)
    }

    /// An entry's name with its root's label in front: `Courses/COMP3670/Exam.pdf`.
    pub fn labelled(&self, entry: &Entry) -> String {
        format!("{}/{}", self.roots[entry.root].label, entry.rel)
    }

    /// The root an index or label names.
    pub fn root_named(&self, label: &str) -> Option<usize> {
        self.roots
            .iter()
            .position(|r| r.label == label)
            .or_else(|| self.roots.iter().position(|r| r.label.eq_ignore_ascii_case(label)))
    }

    /// The entry a labelled name names.
    pub fn get(&self, labelled: &str) -> Option<&Entry> {
        let (label, rel) = labelled.split_once('/')?;
        let root = self.root_named(label)?;
        let at = self.entries.binary_search_by(|e| (e.root, e.rel.as_str()).cmp(&(root, rel))).ok()?;
        Some(&self.entries[at])
    }

    /// The entries of one root under a folder (`""` for the whole root), as a
    /// slice: sorted by path, everything under a folder sits together.
    fn under(&self, root: usize, dir: &str) -> &[Entry] {
        let prefix = match dir.trim_matches('/') {
            "" => String::new(),
            dir => format!("{dir}/"),
        };
        let start = self.entries.partition_point(|e| (e.root, e.rel.as_str()) < (root, prefix.as_str()));
        let end = start + self.entries[start..].partition_point(|e| e.root == root && e.rel.starts_with(&prefix));
        &self.entries[start..end]
    }

    /// A folder's direct contents: its folders, with what is under each, and
    /// its files — and what was left out of it.
    pub fn list(&self, root: usize, dir: &str) -> Listing<'_> {
        let dir = dir.trim_matches('/');
        let skip = if dir.is_empty() { 0 } else { dir.len() + 1 };
        let mut folders: Vec<Folder> = Vec::new();
        let mut files = Vec::new();
        for entry in self.under(root, dir) {
            let rest = &entry.rel[skip..];
            match rest.split_once('/') {
                None => files.push(entry),
                Some((name, _)) => {
                    if folders.last().is_none_or(|f| f.name != name) {
                        folders.push(Folder { name: name.to_string(), files: 0, readable: 0, dataless: 0 });
                    }
                    let folder = folders.last_mut().expect("just pushed");
                    folder.files += 1;
                    folder.readable += usize::from(entry.kind.indexed());
                    folder.dataless += usize::from(entry.dataless);
                }
            }
        }
        let mut skipped = Vec::new();
        for left in self.skipped.iter().filter(|s| s.root == root && !s.rel.is_empty()) {
            let Some(rest) = (if dir.is_empty() { Some(left.rel.as_str()) } else { left.rel.strip_prefix(dir).and_then(|r| r.strip_prefix('/')) }) else {
                continue;
            };
            match rest.split_once('/') {
                None => skipped.push(left),
                // Deeper down: the folder it is in is shown, even with nothing
                // else in it, so what was left out can be found.
                Some((name, _)) => {
                    if !folders.iter().any(|f| f.name == name) {
                        folders.push(Folder { name: name.to_string(), files: 0, readable: 0, dataless: 0 });
                    }
                }
            }
        }
        folders.sort_by(|a, b| a.name.cmp(&b.name));
        Listing { folders, files, skipped }
    }

    /// Files whose labelled name holds every word of `query`, best first: the
    /// name itself matching before a folder above it does, then shorter.
    pub fn find(&self, query: &str, limit: usize) -> Vec<&Entry> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut found: Vec<(bool, usize, &Entry)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let name = self.labelled(entry).to_lowercase();
                if !words.iter().all(|w| name.contains(w.as_str())) {
                    return None;
                }
                let file = name.rsplit('/').next().unwrap_or(&name);
                let in_name = words.iter().all(|w| file.contains(w.as_str()));
                Some((!in_name, name.len(), entry))
            })
            .collect();
        found.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then_with(|| (a.2.root, &a.2.rel).cmp(&(b.2.root, &b.2.rel))));
        found.into_iter().take(limit).map(|(_, _, e)| e).collect()
    }

    /// The files a library of the whole knowledge base reads: searchable kinds,
    /// on the disk, not too big.
    pub fn readable(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.kind.indexed() && !e.dataless && e.size <= e.kind.largest())
    }

    /// How many files, and the newest change among them: two numbers that
    /// move when a file is added, removed or edited.
    pub fn stamp(&self) -> (usize, u64) {
        (self.entries.len(), self.entries.iter().map(|e| e.mtime).max().unwrap_or(0))
    }
}

/// One root's walk.
struct Walker<'a> {
    root: usize,
    inner: &'a [PathBuf],
    rules: &'a RootRules,
    dataset: usize,
    flags: Flags<'a>,
    entries: &'a mut Vec<Entry>,
    skipped: &'a mut Vec<Skipped>,
}

impl Walker<'_> {
    /// List one folder and everything under it. Answers how many files are
    /// under it and how many of them are worth reading, which is what decides
    /// whether the folder above it is a dataset.
    fn visit(&mut self, dir: &Path, rel: &str) -> (usize, usize) {
        let listing = match std::fs::read_dir(dir) {
            Ok(listing) => listing,
            Err(err) => {
                self.skipped.push(Skipped { root: self.root, rel: rel.to_string(), dir: true, why: Why::Unlistable(err.to_string()) });
                return (0, 0);
            }
        };
        let (mut files, mut readable) = (0, 0);
        for item in listing.flatten() {
            let name = item.file_name();
            let Some(name) = name.to_str() else {
                // Not UTF-8: a name that cannot be written into a citation or a
                // settings file, and so cannot be picked.
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = item.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            // `lstat`, never following a link, and never opening the file.
            let Ok(meta) = item.metadata() else { continue };
            let child = match rel {
                "" => name.to_string(),
                rel => format!("{rel}/{name}"),
            };
            let path = item.path();
            if kind.is_dir() {
                if self.inner.contains(&path) {
                    continue;
                }
                if let Some(why) = self.rules.why(&child, true) {
                    self.skipped.push(Skipped { root: self.root, rel: child, dir: true, why });
                    continue;
                }
                if (self.flags)(&path, &meta) & dataless::SF_DATALESS != 0 {
                    self.skipped.push(Skipped { root: self.root, rel: child, dir: true, why: Why::InCloud });
                    continue;
                }
                let (entries, skipped) = (self.entries.len(), self.skipped.len());
                let (n, read) = self.visit(&path, &child);
                if n >= self.dataset && read == 0 {
                    self.entries.truncate(entries);
                    self.skipped.truncate(skipped);
                    self.skipped.push(Skipped { root: self.root, rel: child, dir: true, why: Why::Dataset { files: n } });
                }
                files += n;
                readable += read;
            } else if kind.is_file() {
                if let Some(why) = self.rules.why(&child, false) {
                    self.skipped.push(Skipped { root: self.root, rel: child, dir: false, why });
                    continue;
                }
                let file = Kind::of(Path::new(name));
                files += 1;
                readable += usize::from(file.indexed());
                self.entries.push(Entry {
                    root: self.root,
                    rel: child,
                    kind: file,
                    size: meta.len(),
                    mtime: meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_secs()),
                    dataless: (self.flags)(&path, &meta) & dataless::SF_DATALESS != 0,
                });
            }
        }
        (files, readable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("proton-catalog-{name}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn rels(catalog: &Catalog) -> Vec<String> {
        catalog.entries().iter().map(|e| catalog.labelled(e)).collect()
    }

    #[test]
    fn every_file_is_listed_with_its_kind_and_size() {
        let dir = tree("list");
        write(&dir, "COMP3670/Exam_2020.pdf", "%PDF");
        write(&dir, "COMP3670/notes.md", "# Notes\n");
        write(&dir, "COMP3670/lab1.ipynb", "{}");
        write(&dir, "loose.txt", "hello");
        let catalog = Catalog::walk(&[Root::new("Courses", &dir)], &Rules::default());
        assert_eq!(rels(&catalog), ["Courses/COMP3670/Exam_2020.pdf", "Courses/COMP3670/lab1.ipynb", "Courses/COMP3670/notes.md", "Courses/loose.txt"]);
        let exam = catalog.get("Courses/COMP3670/Exam_2020.pdf").unwrap();
        assert_eq!((exam.kind, exam.size, exam.dataless), (Kind::Pdf, 4, false));
        assert!(exam.mtime > 0);
        assert_eq!(catalog.path(exam), dir.join("COMP3670/Exam_2020.pdf"));
    }

    #[test]
    fn vendor_and_personal_folders_are_left_out_and_said_to_be() {
        let dir = tree("rules");
        write(&dir, "notes/keep.md", "# Keep\n");
        write(&dir, "site/node_modules/pkg/README.md", "# vendor\n");
        write(&dir, "07 - Business/05 - Finance/Tax.md", "# private\n");
        write(&dir, "07 - Business/01 - Strategy/Plan.md", "# fine\n");
        let rules = Rules::new(["Vault/07 - Business/05 - Finance/"]);
        let catalog = Catalog::walk(&[Root::new("Vault", &dir)], &rules);
        assert_eq!(rels(&catalog), ["Vault/07 - Business/01 - Strategy/Plan.md", "Vault/notes/keep.md"]);
        let whys: Vec<(&str, &Why)> = catalog.skipped().iter().map(|s| (s.rel.as_str(), &s.why)).collect();
        assert_eq!(
            whys,
            [
                ("07 - Business/05 - Finance", &Why::Yours("Vault/07 - Business/05 - Finance/".into())),
                ("site/node_modules", &Why::Default("node_modules/".into())),
            ]
        );
    }

    #[test]
    fn a_folder_of_thousands_of_unreadable_files_is_a_dataset() {
        let dir = tree("dataset");
        for i in 0..6 {
            write(&dir, &format!("ir/q1/gov/documents/G{i:04}"), "<DOC>");
        }
        write(&dir, "ir/q1/gov/README.md", "# The collection\n");
        write(&dir, "ir/q1/spec.pdf", "%PDF");
        let catalog = Catalog::walk(&[Root::new("Courses", &dir)], &Rules::default().with_dataset(5));
        assert_eq!(rels(&catalog), ["Courses/ir/q1/gov/README.md", "Courses/ir/q1/spec.pdf"]);
        assert_eq!(catalog.skipped().len(), 1);
        assert_eq!(catalog.skipped()[0].rel, "ir/q1/gov/documents");
        assert_eq!(catalog.skipped()[0].why, Why::Dataset { files: 6 });
    }

    #[test]
    fn strange_names_survive() {
        let dir = tree("names");
        // Colons and newlines are names a course's files really have, and
        // ones Windows will not make.
        let mut names = vec!["Übung — Lösung.pdf", "a#b [c] (d).txt", "Week 3 – Search.md"];
        if cfg!(unix) {
            names.extend(["Lecture 4: Trees.md", "line\nbreak.md"]);
        }
        for name in &names {
            write(&dir, &format!("odd/{name}"), "words");
        }
        let catalog = Catalog::walk(&[Root::new("Courses", &dir)], &Rules::default());
        assert_eq!(catalog.entries().len(), names.len(), "{:?}", rels(&catalog));
        for entry in catalog.entries() {
            assert!(catalog.path(entry).is_file(), "{:?} does not lead back to its file", entry.rel);
            assert_eq!(catalog.get(&catalog.labelled(entry)), Some(entry));
        }
    }

    #[test]
    fn a_file_in_icloud_is_listed_and_a_folder_in_icloud_is_not_opened() {
        let dir = tree("icloud");
        write(&dir, "Year 1/offloaded.pdf", "%PDF");
        write(&dir, "Year 1/here.md", "# Here\n");
        write(&dir, "Year 2/inside.md", "# never listed\n");
        let in_cloud = |path: &Path, _: &std::fs::Metadata| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "offloaded.pdf" || name == "Year 2" {
                dataless::SF_DATALESS
            } else {
                0
            }
        };
        let catalog = Catalog::walk_with(&[Root::new("ANU", &dir)], &Rules::default(), &in_cloud);
        assert_eq!(rels(&catalog), ["ANU/Year 1/here.md", "ANU/Year 1/offloaded.pdf"]);
        assert!(catalog.get("ANU/Year 1/offloaded.pdf").unwrap().dataless);
        assert!(!catalog.get("ANU/Year 1/here.md").unwrap().dataless);
        assert_eq!(catalog.skipped()[0].rel, "Year 2");
        assert_eq!(catalog.skipped()[0].why, Why::InCloud);
        // And what a library reads is what is on the disk.
        let read: Vec<&str> = catalog.readable().map(|e| e.rel.as_str()).collect();
        assert_eq!(read, ["Year 1/here.md"]);
    }

    #[test]
    #[cfg(unix)]
    fn links_are_never_followed_and_hidden_things_never_listed() {
        let dir = tree("links");
        let outside = tree("links-outside");
        write(&outside, "secret.md", "the diagnosis");
        write(&dir, "real.md", "# Real\n");
        write(&dir, ".obsidian/workspace.md", "# machinery\n");
        std::os::unix::fs::symlink(&outside, dir.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.md"), dir.join("secret.md")).unwrap();
        let catalog = Catalog::walk(&[Root::new("Vault", &dir)], &Rules::default());
        assert_eq!(rels(&catalog), ["Vault/real.md"]);
    }

    #[test]
    fn a_missing_root_is_said_and_a_root_can_be_left_out_whole() {
        let dir = tree("missing");
        write(&dir, "a.md", "# A\n");
        let roots = [Root::new("Gone", dir.join("nowhere")), Root::new("ANU", &dir)];
        let catalog = Catalog::walk(&roots, &Rules::new(["ANU/"]));
        assert_eq!(catalog.missing(), [&roots[0]]);
        assert!(catalog.entries().is_empty());
        assert_eq!(catalog.skipped()[0].why, Why::Yours("ANU/".into()));
    }

    #[test]
    fn a_folder_lists_its_folders_with_their_counts_and_its_files() {
        let dir = tree("listing");
        write(&dir, "COMP3670/Week 1/slides.pdf", "%PDF");
        write(&dir, "COMP3670/Week 1/code.py", "print()");
        write(&dir, "COMP3670/Week 2/notes.md", "# N\n");
        write(&dir, "COMP3670/outline.md", "# Outline\n");
        write(&dir, "COMP3670/web/node_modules/x.md", "# vendor\n");
        write(&dir, "COMP2310/a.md", "# A\n");
        let catalog = Catalog::walk(&[Root::new("Courses", &dir)], &Rules::default());

        let top = catalog.list(0, "");
        let names: Vec<(&str, usize, usize)> = top.folders.iter().map(|f| (f.name.as_str(), f.files, f.readable)).collect();
        assert_eq!(names, [("COMP2310", 1, 1), ("COMP3670", 4, 3)]);
        assert!(top.files.is_empty());

        let course = catalog.list(0, "COMP3670/");
        let names: Vec<(&str, usize)> = course.folders.iter().map(|f| (f.name.as_str(), f.files)).collect();
        // `web` holds only what was left out, and is shown so it can be found.
        assert_eq!(names, [("Week 1", 2), ("Week 2", 1), ("web", 0)]);
        assert_eq!(course.files.iter().map(|e| e.rel.as_str()).collect::<Vec<_>>(), ["COMP3670/outline.md"]);
        assert!(course.skipped.is_empty(), "the vendor folder is under web/, not here");
        assert_eq!(catalog.list(0, "COMP3670/web").skipped.len(), 1);
        assert_eq!(catalog.list(0, "COMP3670/Week 1").files.len(), 2);
    }

    #[test]
    fn a_file_is_found_by_the_words_of_its_name() {
        let dir = tree("find");
        write(&dir, "COMP3670/Exam_2020.pdf", "%PDF");
        write(&dir, "COMP3670/Exam_2021_solutions.pdf", "%PDF");
        write(&dir, "Past papers/COMP3670_Exam_2019.pdf", "%PDF");
        write(&dir, "COMP2310/Exam.pdf", "%PDF");
        let catalog = Catalog::walk(&[Root::new("Courses", &dir)], &Rules::default());
        let found: Vec<&str> = catalog.find("comp3670 exam", 10).iter().map(|e| e.rel.as_str()).collect();
        // The one whose own name holds both words first; then shorter first.
        assert_eq!(found, ["Past papers/COMP3670_Exam_2019.pdf", "COMP3670/Exam_2020.pdf", "COMP3670/Exam_2021_solutions.pdf"]);
        assert_eq!(catalog.find("solutions 2021", 10).len(), 1);
        assert!(catalog.find("   ", 10).is_empty());
    }

    #[test]
    fn a_root_inside_another_or_named_twice_is_listed_once() {
        let dir = tree("nested");
        write(&dir, "COMP3670/notes.md", "# N\n");
        write(&dir, "other.md", "# O\n");
        let roots = [Root::new("Courses", &dir), Root::new("COMP3670", dir.join("COMP3670")), Root::new("Again", &dir)];
        let catalog = Catalog::walk(&roots, &Rules::default());
        assert_eq!(rels(&catalog), ["Courses/other.md", "COMP3670/notes.md"]);
    }

    #[test]
    fn roots_are_labelled_by_their_last_folder_and_never_twice() {
        let roots = Root::labelled([PathBuf::from("/Users/x/OnyxVault"), PathBuf::from("/Users/x/Courses"), PathBuf::from("/Volumes/y/Courses")]);
        let labels: Vec<&str> = roots.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["OnyxVault", "Courses", "Courses 2"]);
    }

    #[test]
    fn the_stamp_moves_when_a_file_changes() {
        let dir = tree("stamp");
        write(&dir, "a.md", "# A\n");
        let roots = [Root::new("V", &dir)];
        let one = Catalog::walk(&roots, &Rules::default()).stamp();
        write(&dir, "b.md", "# B\n");
        let two = Catalog::walk(&roots, &Rules::default()).stamp();
        assert_ne!(one, two);
        assert_eq!(two.0, 2);
    }
}
