//! The file tree in the sidebar.
//!
//! Held as *which folders are open* and nothing else. The rows are computed
//! from that and the disk whenever anyone asks, so there is no cached listing
//! to go stale — and a file created by a `cargo new` in another window shows up
//! the next time the tree is drawn rather than never.
//!
//! That is affordable because a tree only ever shows what has been expanded:
//! the cost is one directory read per open folder, not a walk. `␣ff` is the
//! thing that walks, and it does it once on a worker.
//!
//! Listing goes through `proton_index::children`, which applies the same ignore
//! rules the walk does. A tree offering `target/` while `␣ff` refuses to find
//! anything in it would be two different answers to "what is in this project".

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How many entries to show in one folder.
///
/// A directory of ten thousand generated files would otherwise put ten
/// thousand rows over the bridge, none of which anybody is going to read. The
/// row that says so is more useful than the ten thousandth name.
const PER_FOLDER: usize = 500;

/// One line in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct TreeRow {
    /// How deep to indent. The root's own children are 0.
    pub depth: usize,
    pub name: String,
    /// Relative to the project root, forward slashes on every platform — the
    /// same spelling `openBuffer` takes.
    pub path: String,
    pub dir: bool,
    /// Open, for a folder. Meaningless for a file.
    pub expanded: bool,
    /// This is the buffer you are looking at.
    pub active: bool,
    /// A folder with more in it than the tree will show. Carries how many were
    /// left out, so the row can say.
    pub truncated: usize,
}

#[derive(Debug, Default)]
pub struct Tree {
    /// Canonical, and without the `\\?\` canonicalising adds on Windows — the
    /// spelling the editor gives every file it opens. The root as handed over
    /// need not be: the project a session starts with is rooted wherever it
    /// was named, `.` for `proton .`, and one reached through a link is named
    /// by the link, which on a Mac is anything under `/tmp` or `/var`. Against
    /// that spelling the file you are in would be outside the project —
    /// unmarked, and not revealed by `␣e`.
    ///
    /// Only the tree's copy is canonical. The workspace keeps the name it was
    /// given, because that is where the terminal starts and what the project
    /// is called; the rows are relative, so nothing drawn here shows which
    /// spelling the tree used.
    root: PathBuf,
    /// The folders the user has opened. Everything else is closed, which is
    /// the whole of the tree's state.
    ///
    /// A `BTreeSet` so the set has a stable order when it is written out, and
    /// because reopening a project should find the same folders open.
    expanded: BTreeSet<String>,
}

impl Tree {
    pub fn new(root: impl AsRef<Path>) -> Tree {
        Tree {
            root: crate::folder::normalise(root.as_ref()),
            expanded: BTreeSet::new(),
        }
    }

    /// Open a folder, or close it if it was open. `false` if the path is not a
    /// folder in this project.
    pub fn toggle(&mut self, path: &str) -> bool {
        let path = normal(path);
        if !self.root.join(&path).is_dir() {
            return false;
        }
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
        true
    }

    /// Open every folder on the way to a file, so revealing it can scroll to
    /// a row that exists.
    pub fn reveal(&mut self, path: &Path) {
        // Spelled the way the root is, or a file in the project can look like
        // one outside it.
        let path = crate::folder::normalise(path);
        let Ok(relative) = path.strip_prefix(&self.root) else {
            // A file outside the project has no row to reveal, and forcing one
            // would mean a tree that is not a tree of this project.
            return;
        };
        let mut so_far = PathBuf::new();
        // Every component but the last, which is the file itself.
        let mut parts: Vec<_> = relative.components().collect();
        parts.pop();
        for part in parts {
            so_far.push(part);
            self.expanded.insert(normal(&so_far.to_string_lossy()));
        }
    }

    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(&normal(path))
    }

    /// The visible rows, top to bottom.
    ///
    /// `active` is the file being edited, so the row for it can say so — the
    /// tree is also the answer to "where am I", and a tree that cannot say is
    /// half a tree.
    pub fn rows(&self, active: Option<&Path>) -> Vec<TreeRow> {
        // Usually canonical already, since the editor canonicalises what it
        // opens — but not a name given to `:saveas`, which is joined to the
        // workspace's root and kept as it comes out.
        let active = active.map(crate::folder::normalise);
        let mut rows = Vec::new();
        self.walk(&self.root.clone(), 0, active.as_deref(), &mut rows);
        rows
    }

    fn walk(&self, dir: &Path, depth: usize, active: Option<&Path>, rows: &mut Vec<TreeRow>) {
        let found = proton_index::children(&self.root, dir);
        let truncated = found.len().saturating_sub(PER_FOLDER);

        for child in found.into_iter().take(PER_FOLDER) {
            let Ok(relative) = child.path.strip_prefix(&self.root) else {
                continue;
            };
            let path = normal(&relative.to_string_lossy());
            let expanded = child.dir && self.expanded.contains(&path);

            rows.push(TreeRow {
                depth,
                name: child.name,
                path: path.clone(),
                dir: child.dir,
                expanded,
                active: !child.dir && active.is_some_and(|a| a == child.path),
                truncated: 0,
            });

            if expanded {
                self.walk(&child.path, depth + 1, active, rows);
            }
        }

        if truncated > 0 {
            // Attached to the folder that overflowed rather than reported
            // globally, because which folder it was is the useful half.
            rows.push(TreeRow {
                depth,
                name: format!("{truncated} more"),
                path: String::new(),
                dir: false,
                expanded: false,
                active: false,
                truncated,
            });
        }
    }
}

/// One spelling for a path: relative, forward slashes, no leading `./`.
///
/// The tree compares paths against a set and hands them to `openBuffer`, and
/// two spellings of one folder would mean a folder that will not close.
fn normal(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "proton-tree-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        std::fs::write(root.join("src/main.rs"), "").unwrap();
        std::fs::write(root.join("src/deep/inner.rs"), "").unwrap();
        std::fs::write(root.join("target/junk.o"), "").unwrap();
        std::fs::write(root.join(".gitignore"), "target\n").unwrap();
        root
    }

    fn names(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    }

    fn marked(rows: &[TreeRow]) -> Vec<&str> {
        rows.iter()
            .filter(|r| r.active)
            .map(|r| r.name.as_str())
            .collect()
    }

    #[test]
    fn a_closed_tree_shows_only_the_top() {
        let root = scratch("closed");
        let tree = Tree::new(&root);
        assert_eq!(names(&tree.rows(None)), vec!["src", "Cargo.toml"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn folders_come_before_files() {
        // Forty years of file trees, and the order people scan without
        // thinking about it.
        let root = scratch("order");
        let tree = Tree::new(&root);
        let rows = tree.rows(None);
        assert!(rows[0].dir && !rows[1].dir, "{:?}", names(&rows));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ignored_and_hidden_folders_are_not_in_it() {
        // The same rules `␣ff` uses. Two different answers to "what is in this
        // project" would leave the user deciding which to believe.
        let root = scratch("ignored");
        let tree = Tree::new(&root);
        let listed = names(&tree.rows(None));
        assert!(!listed.iter().any(|n| n.contains("target")), "{listed:?}");
        assert!(!listed.iter().any(|n| n.contains(".git")), "{listed:?}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn opening_a_folder_shows_what_is_in_it() {
        let root = scratch("open");
        let mut tree = Tree::new(&root);
        assert!(tree.toggle("src"));
        assert_eq!(
            names(&tree.rows(None)),
            vec!["src", "  deep", "  main.rs", "Cargo.toml"]
        );

        // And closing it puts it back.
        assert!(tree.toggle("src"));
        assert_eq!(names(&tree.rows(None)), vec!["src", "Cargo.toml"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_path_spelled_two_ways_is_one_folder() {
        // The set is keyed by path, so two spellings would mean a folder that
        // opens and then will not close.
        let root = scratch("spelling");
        let mut tree = Tree::new(&root);
        tree.toggle("src");
        assert!(tree.is_expanded("src"));
        assert!(tree.is_expanded("./src"));
        assert!(tree.is_expanded("src/"));
        assert!(tree.is_expanded(r"src\"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn toggling_something_that_is_not_a_folder_does_nothing() {
        let root = scratch("notafolder");
        let mut tree = Tree::new(&root);
        assert!(!tree.toggle("Cargo.toml"));
        assert!(!tree.toggle("nope/at/all"));
        assert_eq!(names(&tree.rows(None)), vec!["src", "Cargo.toml"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn revealing_a_file_opens_every_folder_above_it() {
        // Otherwise "show me where I am" scrolls to a row that does not exist.
        let root = scratch("reveal");
        let mut tree = Tree::new(&root);
        tree.reveal(&root.join("src/deep/inner.rs"));
        assert_eq!(
            names(&tree.rows(None)),
            vec!["src", "  deep", "    inner.rs", "  main.rs", "Cargo.toml"]
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_active_file_says_so() {
        let root = scratch("active");
        let mut tree = Tree::new(&root);
        tree.toggle("src");
        let rows = tree.rows(Some(&root.join("src/main.rs")));
        let active: Vec<&str> = rows
            .iter()
            .filter(|r| r.active)
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(active, vec!["main.rs"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_file_you_are_in_is_found_however_the_project_was_named() {
        // The editor canonicalises what it opens, and the root a tree is given
        // need not be canonical. Compared as given, the open file was outside
        // the project: never marked, and `␣e` revealed nothing. A `..` stands
        // in for `proton .` and for a link, because it needs neither the
        // working directory changed under the other tests nor, on Windows, the
        // privilege a link does.
        let root = scratch("named");
        let named = root.join("src/..");
        let mut editor = crate::editor::Editor::new();
        editor.open(named.join("src/deep/inner.rs")).unwrap();
        let opened = editor.buffer().path().unwrap();

        let mut tree = Tree::new(&named);
        tree.reveal(opened);
        let rows = tree.rows(Some(opened));
        assert_eq!(
            names(&rows),
            vec!["src", "  deep", "    inner.rs", "  main.rs", "Cargo.toml"]
        );
        assert_eq!(marked(&rows), vec!["inner.rs"]);

        // And spelled the root's way, which is what a name given to `:saveas`
        // keeps.
        let rows = tree.rows(Some(&named.join("src/main.rs")));
        assert_eq!(marked(&rows), vec!["main.rs"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_project_reached_through_a_link_still_knows_where_you_are() {
        // How this was found: on a Mac the temporary folder is under `/var`,
        // which is a link into `/private`. Given a link of its own here, so it
        // is tested where `/tmp` is a real folder too.
        let root = scratch("linked");
        let link = PathBuf::from(format!("{}-link", root.display()));
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&root, &link).unwrap();
        let mut editor = crate::editor::Editor::new();
        editor.open(link.join("src/main.rs")).unwrap();
        let opened = editor.buffer().path().unwrap();

        let mut tree = Tree::new(&link);
        tree.reveal(opened);
        assert_eq!(marked(&tree.rows(Some(opened))), vec!["main.rs"]);
        std::fs::remove_file(&link).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_file_outside_the_project_reveals_nothing_rather_than_inventing_a_row() {
        let root = scratch("outside");
        let mut tree = Tree::new(&root);
        tree.reveal(Path::new(r"C:\somewhere\else\x.rs"));
        assert_eq!(names(&tree.rows(None)), vec!["src", "Cargo.toml"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_folder_with_too_much_in_it_says_how_much_was_left_out() {
        // Ten thousand generated files is ten thousand rows over the bridge,
        // none of which anybody reads. The count is the useful part.
        let root = scratch("toomany");
        std::fs::create_dir_all(root.join("many")).unwrap();
        for i in 0..PER_FOLDER + 7 {
            std::fs::write(root.join("many").join(format!("f{i}.txt")), "").unwrap();
        }
        let mut tree = Tree::new(&root);
        tree.toggle("many");

        let rows = tree.rows(None);
        let overflow = rows.iter().find(|r| r.truncated > 0).expect("a count row");
        assert_eq!(overflow.truncated, 7);
        assert_eq!(overflow.name, "7 more");
        std::fs::remove_dir_all(&root).ok();
    }
}
