//! Notebook's media, for the window to play: what `study://localhost/<path>`
//! names, and the bytes of it a player asks for.
//!
//! Two kinds of file and nothing else: a notebook's audio overview,
//! `<slug>/A001.mp3`, which is `.notebook/audio/A001.mp3` in that notebook;
//! and a voice's sample, `_voices/<id>.mp3`, kept in Proton's cache. Every
//! part of a path is a plain name — letters, digits, `-`, `_` and one `.` —
//! and what it names must be a file under its folder once links are
//! followed. A player seeking asks for a range of bytes (`Range:
//! bytes=1000-`), and is answered 206 with that range, a piece at a time.

use std::path::{Path, PathBuf};

/// A range asked and the answer to it, as artifacts are answered.
pub use proton_claude::artifacts::{range, Served};

/// Where voices' samples are kept, or `None` if the platform will not say.
pub fn voices_dir() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("PROTON_VOICE_CACHE") {
        let dir = PathBuf::from(named);
        return std::fs::create_dir_all(&dir).ok().map(|()| dir);
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    }?;
    let dir = base.join("Proton").join("voices");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// A plain name: no separators, no leading dot, nothing a path could climb by.
fn plain(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.len() <= 120 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && !name.contains("..")
}

/// What a `study://` path names, if it names anything that may be played.
pub fn resolve(folder: Option<&Path>, voices: Option<&Path>, path: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let [first, file] = parts.as_slice() else { return None };
    if !plain(first) || !plain(file) || !matches!(Path::new(file).extension().and_then(|e| e.to_str()), Some("mp3" | "m4a" | "wav")) {
        return None;
    }
    let (base, candidate) = match *first {
        "_voices" => {
            let base = voices?;
            (base.to_path_buf(), base.join(file))
        }
        slug => {
            let base = folder?.join(slug).join(".notebook/audio");
            (base.clone(), base.join(file))
        }
    };
    let found = candidate.canonicalize().ok()?;
    let base = base.canonicalize().ok()?;
    (found.starts_with(&base) && found.is_file()).then_some(found)
}

fn content_type(file: &Path) -> &'static str {
    match file.extension().and_then(|e| e.to_str()) {
        Some("m4a") => "audio/mp4",
        Some("wav") => "audio/wav",
        _ => "audio/mpeg",
    }
}

/// Answer `study://localhost/<path>`: the file, or the range of it asked for.
pub fn serve(folder: Option<&Path>, voices: Option<&Path>, path: &str, range_header: Option<&str>) -> Served {
    let missing = || Served { status: 404, headers: vec![("Content-Type", "text/plain; charset=utf-8".into())], body: b"Not kept here.".to_vec() };
    let Some(file) = resolve(folder, voices, path) else { return missing() };
    match proton_claude::artifacts::answer(&file, content_type(&file), range_header) {
        Some(mut served) => {
            served.headers.push(("Cache-Control", "no-cache".to_string()));
            served
        }
        None => missing(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("proton-study-media-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("Notebooks");
        std::fs::create_dir_all(folder.join("search/.notebook/audio")).unwrap();
        std::fs::write(folder.join("search/.notebook/audio/A001.mp3"), (0..=255u8).cycle().take(10_000).collect::<Vec<_>>()).unwrap();
        std::fs::write(folder.join("search/notebook.json"), "{}").unwrap();
        let voices = root.join("voices");
        std::fs::create_dir_all(&voices).unwrap();
        std::fs::write(voices.join("v1.mp3"), b"sample").unwrap();
        (folder, voices)
    }

    #[test]
    fn only_a_notebooks_audio_and_a_voices_sample_are_named() {
        let (folder, voices) = scratch();
        let (f, v) = (Some(folder.as_path()), Some(voices.as_path()));
        assert!(resolve(f, v, "/search/A001.mp3").is_some());
        assert!(resolve(f, v, "/_voices/v1.mp3").is_some());
        for wrong in ["/search/notebook.json", "/search/../search/A001.mp3", "/../Notebooks/search/A001.mp3", "/search/A002.mp3", "/search/.notebook/A001.mp3", "/A001.mp3", "/search/sub/A001.mp3", "/.hidden/A001.mp3"] {
            assert!(resolve(f, v, wrong).is_none(), "{wrong} was served");
        }
        // A link out of the folder is not followed out.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(folder.join("search/notebook.json"), folder.join("search/.notebook/audio/A009.mp3")).unwrap();
            assert!(resolve(f, v, "/search/A009.mp3").is_none(), "a link to a file outside the audio folder was served");
        }
        assert!(resolve(None, v, "/search/A001.mp3").is_none(), "no folder, no notebooks");
    }

    #[test]
    fn a_player_seeking_is_answered_with_the_range_it_asked_for() {
        assert_eq!(range(Some("bytes=0-"), 100), Some((0, 99)));
        assert_eq!(range(Some("bytes=10-19"), 100), Some((10, 19)));
        assert_eq!(range(Some("bytes=90-500"), 100), Some((90, 99)), "an end past the file is its end");
        assert_eq!(range(Some("bytes=-30"), 100), Some((70, 99)));
        assert_eq!(range(Some("bytes=100-"), 100), None, "past the end");
        assert_eq!(range(Some("items=0-1"), 100), None);
        assert_eq!(range(None, 100), None);
        let (folder, voices) = scratch();
        let (f, v) = (Some(folder.as_path()), Some(voices.as_path()));
        let whole = serve(f, v, "/search/A001.mp3", None);
        assert_eq!((whole.status, whole.body.len()), (200, 10_000));
        assert!(whole.headers.contains(&("Accept-Ranges", "bytes".into())) && whole.headers.contains(&("Content-Type", "audio/mpeg".into())));
        let part = serve(f, v, "/search/A001.mp3", Some("bytes=256-511"));
        assert_eq!((part.status, part.body.len(), part.body[0]), (206, 256, 0), "byte 256 is 0 again");
        assert!(part.headers.contains(&("Content-Range", "bytes 256-511/10000".into())));
        assert_eq!(serve(f, v, "/search/A001.mp3", Some("bytes=20000-")).status, 416);
        assert_eq!(serve(f, v, "/search/nothing.mp3", None).status, 404);
    }
}
