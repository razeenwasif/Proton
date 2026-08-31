use proton_index::{grep, FileIndex, Matcher};

/// The index is built against this repository, which is the honest test: a
/// synthetic tree would not have a `target/` directory to prove we skip.
fn repo() -> FileIndex {
    let mut index = FileIndex::new(std::env::current_dir().unwrap().join("../.."));
    index.scan();
    index
}

#[test]
fn the_walk_finds_source_and_skips_what_gitignore_says_to() {
    let index = repo();
    assert!(index.len() > 20, "only found {} files", index.len());

    let paths = index.paths();
    assert!(paths.iter().any(|p| p == "crates/proton-core/src/vim.rs"));
    assert!(paths.iter().any(|p| p == "ui/app.js"));

    assert!(
        !paths.iter().any(|p| p.starts_with("target/")),
        "target/ is in .gitignore and must not be offered"
    );
    assert!(
        !paths
            .iter()
            .any(|p| p.contains("/.git/") || p.starts_with(".git/")),
        ".git is not in .gitignore, so it is filtered explicitly"
    );
}

#[test]
fn paths_use_forward_slashes_on_every_platform() {
    // A query typed with `/` has to work the same on Windows.
    let index = repo();
    assert!(!index.paths().iter().any(|p| p.contains('\\')));
}

#[test]
fn a_file_name_finds_the_file_wherever_it_lives() {
    let index = repo();
    let mut matcher = Matcher::new();
    let hits = matcher.rank("vimrs", index.paths(), 10);
    let best = &index.paths()[hits[0].index];
    assert_eq!(best, "crates/proton-core/src/vim.rs", "ranked: {hits:?}");
    assert!(
        !hits[0].positions.is_empty(),
        "the UI needs the matched columns"
    );
}

#[test]
fn ties_break_towards_the_shorter_path() {
    let mut matcher = Matcher::new();
    let candidates = vec!["a/b/c/thing.rs".to_string(), "thing.rs".to_string()];
    let hits = matcher.rank("thing", &candidates, 10);
    assert_eq!(candidates[hits[0].index], "thing.rs");
}

#[test]
fn an_empty_query_shows_everything() {
    let index = repo();
    let mut matcher = Matcher::new();
    let hits = matcher.rank("", index.paths(), 5);
    assert_eq!(hits.len(), 5, "a freshly opened picker shows the list");
}

#[test]
fn a_query_that_matches_nothing_returns_nothing() {
    let index = repo();
    let mut matcher = Matcher::new();
    assert!(matcher.rank("zzzqqqxxx", index.paths(), 10).is_empty());
}

#[test]
fn grep_finds_a_line_and_reports_where() {
    let index = repo();
    let hits = grep(index.root(), index.paths(), "pub struct Workspace", 20);
    let hit = hits.first().expect("Workspace is defined somewhere");
    assert!(hit.path.ends_with("workspace.rs"), "found in {}", hit.path);
    assert!(hit.text.contains("pub struct Workspace"));
}

#[test]
fn grep_stops_at_the_limit() {
    let index = repo();
    // `use` appears far more than three times; the point is that it stops.
    assert_eq!(grep(index.root(), index.paths(), "use ", 3).len(), 3);
}

#[test]
fn grep_ignores_case_and_binaries() {
    let index = repo();
    let hits = grep(index.root(), index.paths(), "PUB STRUCT WORKSPACE", 5);
    assert!(!hits.is_empty(), "search is case-insensitive");
    // The PNGs in brand/ are walked but must never produce a hit.
    assert!(!hits.iter().any(|h| h.path.ends_with(".png")));
}
