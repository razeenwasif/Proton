//! Two Rust types with one name silently become one TypeScript file.
//!
//! `ts-rs` writes every exported type into one flat `ui/bindings/` directory,
//! named after the type. Nothing warns when a second crate exports a type with
//! a name already taken — the file is simply overwritten by whichever test ran
//! last, so the build stays green, the bindings are wrong for one of the two,
//! and the front end reads a shape that does not match what the core sends.
//!
//! This crate's scene node was called `Object` for exactly long enough to
//! overwrite `proton-core`'s vim text-object enum. It was caught by `git
//! status` saying "modified" where "new file" was expected, which is not a
//! thing to rely on.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Every `#[ts(export)]` in the workspace, by the file it will be written to.
///
/// Read out of the source rather than by running the generators, because the
/// generators are what overwrite each other — asking them would ask after the
/// damage.
fn exported() -> HashMap<String, Vec<String>> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf();

    let mut found: HashMap<String, Vec<String>> = HashMap::new();
    for file in rust_files(&crates) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mut exporting = false;
        // A `rename` in the attribute is the name the file is written under.
        let mut renamed: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("#[ts(") && line.contains("export") {
                exporting = true;
                renamed = rename(line);
                continue;
            }
            if !exporting || line.starts_with('#') || line.starts_with("///") {
                continue;
            }
            // The declaration the attribute was attached to.
            if let Some(name) = renamed.take().or_else(|| declared(line)) {
                found
                    .entry(name)
                    .or_default()
                    .push(file.display().to_string());
            }
            exporting = false;
        }
    }
    found
}

/// The name out of a `rename = "Foo"` in a `#[ts(...)]` attribute.
fn rename(attribute: &str) -> Option<String> {
    // Every `rename`, skipping a `rename_all` that may come first.
    attribute.match_indices("rename").find_map(|(at, word)| {
        let rest = attribute[at + word.len()..].trim_start().strip_prefix('=')?;
        let rest = rest.trim_start().strip_prefix('"')?;
        let name: String = rest.chars().take_while(|c| *c != '"').collect();
        (!name.is_empty()).then_some(name)
    })
}

/// The type name out of a `pub struct Foo {` or `pub enum Foo {`.
fn declared(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("pub struct ")
        .or_else(|| line.strip_prefix("pub enum "))
        .or_else(|| line.strip_prefix("struct "))
        .or_else(|| line.strip_prefix("enum "))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `target/` under a crate would be a copy of everything.
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out
}

#[test]
fn no_two_exported_types_share_a_name() {
    let clashes: Vec<(String, Vec<String>)> = exported()
        .into_iter()
        .filter(|(_, places)| places.len() > 1)
        .collect();

    assert!(
        clashes.is_empty(),
        "these would overwrite each other in ui/bindings/, and the last test \
         to run would decide which one the front end gets:\n{clashes:#?}"
    );
}

#[test]
fn the_scan_can_actually_see_the_types_it_is_guarding() {
    // A test that finds nothing passes for the wrong reason. This one has to
    // fail if the scan stops working, not just if the names stay unique.
    let all = exported();
    assert!(
        all.len() > 40,
        "only found {} exported types — the scan is broken, not the workspace",
        all.len()
    );
    assert!(all.contains_key("Node"), "this crate's own type is missing");
    assert!(
        all.contains_key("Object"),
        "proton-core's Object is missing"
    );
}
