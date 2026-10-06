//! Folders, as things you can type the name of.
//!
//! Opening a project means naming a directory that is, by definition, not in
//! the project — so none of the machinery that makes `␣ff` fast applies. There
//! is no index to search and nothing to walk in the background, because the
//! answer could be anywhere on the disk.
//!
//! So this completes a path one segment at a time, the way `:e` does: a single
//! `read_dir` of the folder you have typed so far, filtered by the part you
//! have typed of the next name. One directory read per keystroke, whatever the
//! size of the disk — which is the only shape of this that stays instant.

use std::path::{Path, PathBuf};

/// How many completions to offer. More than fits on screen, few enough that a
/// `node_modules` sitting in the way cannot cost a frame.
pub const LIMIT: usize = 200;

/// The user's home directory. Where `~` goes, and where the picker starts when
/// there is nothing else to go on.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

/// Where Proton keeps what it remembers between runs.
///
/// Config rather than data, deliberately: it is a handful of paths, a window
/// size and whatever a plugin brought, and it is the sort of thing somebody
/// should be able to find, read and delete.
///
/// One function rather than the same three branches in each caller — the
/// session, the plugins folder and the window's geometry all live here, and
/// they must agree on where "here" is.
pub fn config() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var_os("APPDATA")?;
        return Some(PathBuf::from(base).join("Proton"));
    }
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(base).join("proton"));
    }
    Some(home()?.join(".config").join("proton"))
}

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Whether a Windows drive was named on its own — `C:`, with no separator.
/// `Path` treats that as "the current directory on C:", which is not what
/// somebody typing it into a folder prompt means.
fn bare_drive(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Does this look like the user is typing a path, rather than a name to search
/// for? A leading `~`, a separator anywhere, or a drive letter.
///
/// The distinction matters because the picker shows two different things: with
/// a name, the projects you have opened before; with a path, what is actually
/// in that folder.
pub fn looks_like_path(query: &str) -> bool {
    query.starts_with('~')
        || query.starts_with('.')
        || query.contains('/')
        || query.contains('\\')
        || bare_drive(query)
}

/// Expand a leading `~` and nothing else. Deliberately not a shell: `$VAR` and
/// globs are not path syntax, and pretending otherwise makes a folder called
/// `$HOME` unopenable.
pub fn expand(query: &str) -> PathBuf {
    let Some(rest) = query.strip_prefix('~') else {
        return PathBuf::from(query);
    };
    let Some(home) = home() else {
        return PathBuf::from(query);
    };
    let rest = rest.trim_start_matches(is_separator);
    if rest.is_empty() {
        home
    } else {
        home.join(rest)
    }
}

/// Split a typed path into the folder to list and the prefix to match in it.
///
/// The trailing separator is the whole signal: `C:\src` means "the things in
/// `C:\` starting with `src`", and `C:\src\` means "everything in `C:\src`".
/// That is what makes the list advance as you type rather than flickering
/// between two directories.
///
/// Done on the text rather than with `Path::parent` and `Path::file_name`,
/// which normalise: they read `src\.` as the *name* `src` inside its parent,
/// so asking for the hidden entries of a folder would list its siblings
/// instead. Here a dot is a prefix like any other character, which is what
/// somebody typing one means.
fn split(query: &str) -> (PathBuf, String) {
    if bare_drive(query) {
        // `C:` on its own is the root of that drive, not the process's
        // current directory on it.
        return (PathBuf::from(format!("{query}\\")), String::new());
    }

    // Split what was *typed*, and expand only the directory half afterwards.
    //
    // Expanding first loses the thing the split depends on: `~/` expands to a
    // home directory with no trailing separator, which then reads as "the name
    // `Razeen`, inside `C:\Users`" — one row, and no way to get inside your own
    // home folder. The trailing separator is the entire signal, so it has to
    // survive to be read.
    match query.rfind(is_separator) {
        // The separator stays with the directory, so a root stays a root:
        // `C:\` must not be trimmed into `C:`, which means something else.
        Some(at) => (expand(&query[..=at]), query[at + 1..].to_string()),
        // `~` alone is a folder, not a half-typed name — nobody is going on to
        // type the rest of their own home directory.
        None if query == "~" => (
            home().unwrap_or_else(|| PathBuf::from(query)),
            String::new(),
        ),
        // `.` and `..` name a folder; anything else with no separator in it is
        // a prefix to match in the current one.
        None if query == "." || query == ".." => (PathBuf::from(query), String::new()),
        None => (PathBuf::from("."), query.to_string()),
    }
}

/// The directories under what has been typed so far, in name order.
///
/// Only directories, because the thing being chosen is a project. Hidden ones
/// are left out until the prefix starts with a dot — a folder prompt that
/// opens with `.git` at the top is a folder prompt nobody reads.
pub fn complete(query: &str, limit: usize) -> Vec<PathBuf> {
    let (dir, prefix) = split(query);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let wanted = prefix.to_lowercase();
    let show_hidden = prefix.starts_with('.');

    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !show_hidden && name.starts_with('.') {
                return false;
            }
            if !name.to_lowercase().starts_with(&wanted) {
                return false;
            }
            // `file_type` comes back with the directory read on both
            // platforms, so this costs nothing. A symlink is the exception and
            // has to be followed to find out.
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => true,
                Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
                _ => false,
            }
        })
        .map(|entry| entry.path())
        .collect();

    found.sort_by_key(|path| name_of(path).to_lowercase());
    found.truncate(limit);
    found
}

/// A path as somebody would write it: under the home folder, from `~`.
pub fn tilde(path: &Path) -> String {
    match home().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// What to call a folder. Its own name, or the whole path for a drive root,
/// which has no name of its own.
pub fn name_of(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

/// Whether a folder is the top of a disk: `/`, or a drive root like `C:\`.
///
/// Nowhere the session should land without being told to. A workspace rooted
/// at one builds its file index by walking everything on the drive.
pub fn is_root(path: &Path) -> bool {
    path.parent().is_none()
}

/// Names for a set of folders, with enough of the parent added to tell any two
/// apart.
///
/// Two checkouts of the same repository, or any two projects with a `src` in
/// them, is the ordinary case rather than the exotic one — and a switcher
/// showing two tabs both called `src` is worse than no switcher.
pub fn disambiguate(paths: &[PathBuf]) -> Vec<String> {
    let plain: Vec<String> = paths.iter().map(|p| name_of(p)).collect();

    plain
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let collides = plain
                .iter()
                .enumerate()
                .any(|(j, other)| j != i && other == name);
            if !collides {
                return name.clone();
            }
            match paths[i].parent() {
                Some(parent) => format!("{}/{}", name_of(parent), name),
                // Nothing left to qualify it with.
                None => paths[i].display().to_string(),
            }
        })
        .collect()
}

/// Tidy a path for storing and comparing: absolute, and without the `\\?\`
/// prefix `canonicalize` adds on Windows, which is correct and unreadable.
///
/// Comparison is why this exists. `C:\src\thing` and `C:\src\thing\` and
/// `.\thing` are one workspace, and without a normal form the session would
/// happily open three.
pub fn normalise(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(real) => proton_lsp::uri::plain(real),
        // Does not exist yet, or cannot be read. Absolute is still better than
        // relative, so the error at least names something findable.
        Err(_) => match std::env::current_dir() {
            Ok(cwd) if path.is_relative() => cwd.join(path),
            _ => path.to_path_buf(),
        },
    }
}

/// Whether a folder holds too much to walk before anybody asks for a file in
/// it: the home folder, a folder with the home folder in it, or the top of a
/// disk.
///
/// The home folder is where the session lands with nothing else to go on, and
/// it is not a project. Walking it reads everything a person keeps on the
/// machine: on a Mac that includes `~/Library`, and the folders the system
/// asks permission for. The folders above it hold more again.
///
/// `home` is given rather than looked up, so that a test can name its own.
/// Both sides are normalised, because a home folder reached through a link or
/// a `..` is still the home folder.
pub fn is_too_broad(path: &Path, home: Option<&Path>) -> bool {
    let path = normalise(path);
    path.parent().is_none() || home.is_some_and(|home| normalise(home).starts_with(&path))
}

/// A project's folder under both of its names.
///
/// One is the name it was opened by: `proton .`, a path through a symlink, or
/// on macOS anything under `/tmp` and `/var`, which point into `/private`. The
/// other is the real folder behind it. The editor resolves every file it
/// opens, so those buffers are spelled through the real folder, while
/// `:w {file}`, `:saveas`, a file staged before it exists and anything
/// resolved against the root are spelled through the name it was opened by.
/// They are files in the same folder, and matching only one spelling loses
/// the other: a review that reads the disk instead of the buffer with your
/// unsaved work in it, or one file open as two buffers.
#[derive(Debug, Clone, Default)]
pub struct Root {
    path: PathBuf,
    /// `path` with its symlinks resolved, worked out once.
    real: PathBuf,
}

impl Root {
    pub fn new(path: impl AsRef<Path>) -> Root {
        let path = path.as_ref().to_path_buf();
        Root {
            real: normalise(&path),
            path,
        }
    }

    /// Where a file is in this folder, under either name, or `None` if it is
    /// not in it.
    pub fn inside<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        path.strip_prefix(&self.path)
            .or_else(|_| path.strip_prefix(&self.real))
            .ok()
    }

    /// Whether two paths are one file: spelled alike, or the same place in
    /// this folder reached through its two names.
    pub fn same(&self, a: &Path, b: &Path) -> bool {
        a == b
            || matches!(
                (self.inside(a), self.inside(b)),
                (Some(here), Some(there)) if here == there
            )
    }

    /// A path as a row shows it: relative to this folder when it is in it,
    /// whole when it is not, and with `/` between the parts on every platform.
    ///
    /// Relative because the folder is the part every row shares, and an
    /// absolute path spends most of a row on it.
    pub fn shown(&self, path: &Path) -> String {
        self.inside(path)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_under_home_is_written_from_tilde() {
        if let Some(home) = home() {
            assert_eq!(tilde(&home), "~");
            let sep = std::path::MAIN_SEPARATOR;
            assert_eq!(tilde(&home.join("Github").join("Research")), format!("~{sep}Github{sep}Research"));
        }
        let elsewhere = if cfg!(windows) { PathBuf::from(r"Z:\elsewhere") } else { PathBuf::from("/elsewhere") };
        assert_eq!(tilde(&elsewhere), elsewhere.display().to_string());
    }

    #[test]
    fn a_name_is_not_a_path_but_a_separator_makes_one() {
        assert!(!looks_like_path("proton"));
        assert!(!looks_like_path(""));
        assert!(looks_like_path("~"));
        assert!(looks_like_path("./thing"));
        assert!(looks_like_path("src/lib"));
        assert!(looks_like_path(r"C:\Users"));
        assert!(looks_like_path("C:"));
    }

    #[test]
    fn tilde_expands_and_nothing_else_does() {
        if let Some(home) = home() {
            assert_eq!(expand("~"), home);
            assert_eq!(expand("~/src"), home.join("src"));
        }
        // Not a shell. A folder really called `$HOME` has to stay openable.
        assert_eq!(expand("$HOME"), PathBuf::from("$HOME"));
    }

    #[test]
    fn the_trailing_separator_decides_which_folder_is_listed() {
        // Without it you are still naming a child of the parent; with it you
        // are inside. Getting this backwards makes the list flicker between
        // two directories as you type the last character of a name.
        let (dir, prefix) = split("projects/Raz");
        assert_eq!(dir, PathBuf::from("projects"));
        assert_eq!(prefix, "Raz");

        let (dir, prefix) = split("projects/");
        assert_eq!(dir, PathBuf::from("projects"));
        assert_eq!(prefix, "");
    }

    #[test]
    fn a_tilde_with_a_separator_goes_inside_home() {
        // The bug this exists to stop: expanding before splitting turns `~/`
        // into a home directory with no trailing separator, which then reads as
        // "the name `Razeen` inside `C:\Users`". The prompt showed one row —
        // your own home folder — and there was no way to get into it.
        let Some(home) = home() else { return };

        for typed in ["~/", "~\\"] {
            let (dir, prefix) = split(typed);
            assert_eq!(dir, home, "{typed}");
            assert_eq!(prefix, "", "{typed}: nothing was typed to match");
        }

        // And the folders inside home are what comes back, not home itself.
        let listed = complete("~/", LIMIT);
        assert!(
            listed
                .iter()
                .all(|path| path.parent() == Some(home.as_path())),
            "everything listed should be a child of home: {listed:?}"
        );
        assert!(
            !listed.contains(&home),
            "and home should not be listed inside itself: {listed:?}"
        );
    }

    // Windows path interpretation belongs on Windows; filesystem tests below
    // use native separators and run on every platform.
    #[cfg(windows)]
    #[test]
    fn windows_paths_accept_both_separator_styles() {
        for query in [r"C:\Users\Raz", "C:/Users/Raz"] {
            let (dir, prefix) = split(query);
            assert_eq!(dir, PathBuf::from(r"C:\Users"));
            assert_eq!(prefix, "Raz");
        }
        let (dir, prefix) = split(r"C:\Users\Razeen\.");
        assert_eq!(dir, PathBuf::from(r"C:\Users\Razeen"));
        assert_eq!(prefix, ".");
    }

    #[test]
    fn a_bare_drive_means_its_root() {
        // `Path` reads `C:` as "the current directory on C:", which is never
        // what somebody typing it into a folder prompt meant.
        let (dir, prefix) = split("C:");
        assert_eq!(dir, PathBuf::from(r"C:\"));
        assert_eq!(prefix, "");
    }

    #[test]
    fn completion_offers_directories_and_only_directories() {
        let root = temp("complete");
        std::fs::create_dir_all(root.join("alpha")).unwrap();
        std::fs::create_dir_all(root.join("beta")).unwrap();
        std::fs::write(root.join("gamma.txt"), "not a folder").unwrap();

        let query = format!("{}{sep}", root.display(), sep = std::path::MAIN_SEPARATOR);
        let names: Vec<String> = complete(&query, LIMIT).iter().map(|p| name_of(p)).collect();
        assert_eq!(names, vec!["alpha", "beta"]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_prefix_narrows_and_ignores_case() {
        let root = temp("prefix");
        std::fs::create_dir_all(root.join("Projects")).unwrap();
        std::fs::create_dir_all(root.join("Pictures")).unwrap();
        std::fs::create_dir_all(root.join("Music")).unwrap();

        let query = root.join("proj").display().to_string();
        let names: Vec<String> = complete(&query, LIMIT).iter().map(|p| name_of(p)).collect();
        assert_eq!(names, vec!["Projects"]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn hidden_folders_wait_to_be_asked_for() {
        // `.git` at the top of every folder listing makes the listing useless.
        let root = temp("hidden");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();

        let all = format!("{}{sep}", root.display(), sep = std::path::MAIN_SEPARATOR);
        let names: Vec<String> = complete(&all, LIMIT).iter().map(|p| name_of(p)).collect();
        assert_eq!(names, vec!["src"]);

        let dotted = format!("{}{sep}.", root.display(), sep = std::path::MAIN_SEPARATOR);
        let names: Vec<String> = complete(&dotted, LIMIT)
            .iter()
            .map(|p| name_of(p))
            .collect();
        assert_eq!(names, vec![".git"]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_trailing_dot_is_a_prefix_not_a_normalised_path() {
        // `Path::file_name` reads `src\.` as the name `src` in the parent
        // directory, so splitting with it would answer a request for a
        // folder's hidden entries with a list of its siblings.
        let (dir, prefix) = split("projects/Razeen/.");
        assert_eq!(dir, PathBuf::from("projects/Razeen"));
        assert_eq!(prefix, ".");
    }

    #[test]
    fn a_folder_that_is_not_there_completes_to_nothing() {
        // Not an error, and not a panic: you are mid-way through typing a path
        // that does not exist yet on every keystroke but the last.
        assert!(complete(r"C:\definitely\not\here\at\all\x", LIMIT).is_empty());
    }

    #[test]
    fn names_gain_a_parent_only_when_they_collide() {
        let paths = vec![
            PathBuf::from("work").join("proton"),
            PathBuf::from("work").join("alpha").join("src"),
            PathBuf::from("work").join("beta").join("src"),
        ];
        assert_eq!(
            disambiguate(&paths),
            vec!["proton", "alpha/src", "beta/src"],
        );
    }

    #[test]
    fn normalising_makes_one_workspace_of_three_spellings() {
        // The session compares roots to decide whether a folder is already
        // open. Without a normal form it would happily open the same project
        // three times under three spellings.
        let root = temp("normal");
        std::fs::create_dir_all(root.join("inner")).unwrap();

        let direct = normalise(&root.join("inner"));
        let dotted = normalise(&root.join(".").join("inner"));
        let around = normalise(&root.join("inner").join("..").join("inner"));
        assert_eq!(direct, dotted);
        assert_eq!(direct, around);
        // And it is readable — `canonicalize` alone hands back a `\\?\` path.
        assert!(!direct.to_string_lossy().starts_with(r"\\?\"), "{direct:?}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_home_folder_and_the_top_of_a_disk_are_too_broad_to_walk_unasked() {
        let root = temp("broad");
        let home = root.join("home");
        let project = home.join("project");
        std::fs::create_dir_all(project.join("sub")).unwrap();
        std::fs::create_dir_all(root.join("hom")).unwrap();
        let top = root.ancestors().last().unwrap();

        // The home folder, spelled the long way round on either side. Through
        // `hom` on the home side, because `home/project/..` begins with the
        // home folder's own name as written, normalised or not.
        assert!(is_too_broad(&home, Some(&home)));
        assert!(is_too_broad(&project.join(".."), Some(&home)));
        let roundabout = root.join("hom").join("..").join("home");
        assert!(is_too_broad(&home, Some(&roundabout)));
        // Anything with the home folder in it, up to the top of the disk —
        // which is too broad with no home folder to compare against, too.
        assert!(is_too_broad(&root, Some(&home)));
        assert!(is_too_broad(top, Some(&home)));
        assert!(is_too_broad(top, None));

        // A project in the home folder is a project, and a folder whose name
        // is only the start of the home folder's has nothing to do with it.
        assert!(!is_too_broad(&project, Some(&home)));
        assert!(!is_too_broad(&project.join("sub").join(".."), Some(&home)));
        assert!(!is_too_broad(&root.join("hom"), Some(&home)));
        assert!(!is_too_broad(&project, None));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_file_is_in_the_folder_under_either_of_its_names() {
        // `proton .` names the project by a dot, and the editor names each
        // file it opens by its real path. A symlink to the project is the same
        // case with a link where the dot is; `tests/staging.rs` makes one.
        let root = Root::new(".");
        let dotted = Path::new(".").join("src").join("lib.rs");
        let real = normalise(Path::new(".")).join("src").join("lib.rs");

        assert_eq!(root.inside(&dotted), Some(Path::new("src/lib.rs")));
        assert_eq!(root.inside(&real), Some(Path::new("src/lib.rs")));
        assert!(root.same(&dotted, &real));
        assert!(!root.same(&dotted, &real.with_file_name("folder.rs")));
        assert_eq!(root.shown(&real), "src/lib.rs");
    }

    #[test]
    fn a_file_outside_the_folder_is_only_itself() {
        let root = Root::new(".");
        let elsewhere = normalise(Path::new("..")).join("elsewhere.rs");
        assert_eq!(root.inside(&elsewhere), None);
        assert!(!root.same(&elsewhere, &normalise(Path::new(".")).join("elsewhere.rs")));
        assert_eq!(
            root.shown(&elsewhere),
            elsewhere.to_string_lossy().replace('\\', "/")
        );

        // And with no folder at all, a path is the same file only as itself.
        let none = Root::default();
        assert!(none.same(&elsewhere, &elsewhere));
        assert!(!none.same(Path::new("/a/x.rs"), Path::new("/b/x.rs")));
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "proton-folder-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
