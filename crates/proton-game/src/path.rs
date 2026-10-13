//! A path a game's files name, held to the game's folder.
//!
//! A room names its images, a game its rooms, and every one of those strings
//! came from a file anybody could have edited — or from the agent. So none is
//! joined to the folder and trusted: it must be plain parts, end in a kind of
//! file a game is made of, and still be inside the folder once symlinks are
//! followed. Proton's artifacts hold their files the same way
//! (`proton-claude`'s `artifacts::plain`); the symlink is the part a string
//! check alone cannot see.

use std::path::{Component, Path, PathBuf};

/// The kinds of file a game is made of.
pub const KINDS: &[&str] = &[
    "json", "png", "jpg", "jpeg", "webp", "psd", "aseprite", "ase", "glb", "gltf", "wav", "mp3", "m4a",
];

/// `rel` inside `root`, refused unless it is plainly a game's file there.
///
/// For a file that is about to be written, which need not exist yet, the
/// nearest folder above it that does exist is what is held to the root.
pub fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if rel.is_empty() || rel.contains('\\') || rel.contains('\0') {
        return Err(format!("{rel:?} is not a path in the game"));
    }
    if !named.components().all(|part| matches!(part, Component::Normal(_))) {
        return Err(format!("{rel} reaches outside the game's folder"));
    }
    let kind = named.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if !KINDS.contains(&kind.as_str()) {
        return Err(format!("{rel} is not a kind of file a game is made of"));
    }
    let base = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let path = base.join(named);
    // The nearest part of it that exists, with its links followed.
    let mut held = path.as_path();
    let real = loop {
        match std::fs::canonicalize(held) {
            Ok(real) => break real,
            Err(_) => match held.parent() {
                Some(up) => held = up,
                None => return Err(format!("{rel} is nowhere")),
            },
        }
    };
    if !real.starts_with(&base) {
        return Err(format!("{rel} leads outside the game's folder"));
    }
    Ok(path)
}
