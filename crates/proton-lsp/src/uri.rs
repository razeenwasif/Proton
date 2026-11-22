//! `file:` URIs, which are the only part of LSP that Windows makes interesting.
//!
//! A server speaks URIs and an editor speaks paths, so every message crosses
//! this boundary twice. `C:\src\main.rs` has to become `file:///C:/src/main.rs`
//! — three slashes, forward slashes, an unencoded drive colon — and a path with
//! a space in it has to survive the trip back unchanged.

use std::path::{Component, Path, PathBuf};

/// Bidirectional host ↔ container path & `file://` URI translator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMap {
    pub host_root: PathBuf,
    pub container_root: String,
}

impl PathMap {
    pub fn new(host_root: impl Into<PathBuf>, container_root: impl Into<String>) -> Self {
        let mut container_root = container_root.into();
        while container_root.len() > 1 && container_root.ends_with('/') {
            container_root.pop();
        }
        Self {
            host_root: host_root.into(),
            container_root,
        }
    }

    pub fn host_to_container(&self, host_path: &Path) -> Option<String> {
        let rel = strip_host_prefix(&self.host_root, host_path)?;
        let mut out = if self.container_root == "/" {
            String::new()
        } else {
            self.container_root.clone()
        };
        for comp in rel.components() {
            match comp {
                Component::Normal(seg) => {
                    out.push('/');
                    out.push_str(&seg.to_string_lossy());
                }
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Some(if out.is_empty() { "/".to_string() } else { out })
    }

    pub fn container_to_host(&self, container_path: &str) -> Option<PathBuf> {
        let trimmed = if container_path.len() > 1 {
            container_path.trim_end_matches('/')
        } else {
            container_path
        };
        let rel = if self.container_root == "/" {
            trimmed.strip_prefix('/')?
        } else if trimmed == self.container_root {
            ""
        } else {
            trimmed
                .strip_prefix(&self.container_root)?
                .strip_prefix('/')?
        };
        let mut out = self.host_root.clone();
        for seg in rel.split('/').filter(|s| !s.is_empty() && *s != ".") {
            if seg == ".." {
                return None;
            }
            out.push(seg);
        }
        Some(out)
    }

    pub fn host_to_uri(&self, host_path: &Path) -> String {
        match self.host_to_container(host_path) {
            Some(cpath) => from_posix(&cpath),
            None => from_path(host_path),
        }
    }

    pub fn uri_to_host_path(&self, uri: &str) -> Option<PathBuf> {
        if let Some(cpath) = to_posix_string(uri) {
            if let Some(host) = self.container_to_host(&cpath) {
                return Some(host);
            }
        }
        to_path(uri)
    }

    pub fn uri_to_host_uri(&self, uri: &str) -> String {
        match to_posix_string(uri).and_then(|c| self.container_to_host(&c)) {
            Some(host) => from_path(&host),
            None => uri.to_string(),
        }
    }
}

fn strip_host_prefix(root: &Path, target: &Path) -> Option<PathBuf> {
    if let Ok(rel) = target.strip_prefix(root) {
        return Some(rel.to_path_buf());
    }
    let canon_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if let Ok(rel) = target.strip_prefix(&canon_root) {
        return Some(rel.to_path_buf());
    }
    if let Some(canon_target) = canonicalize_with_tail(target) {
        if let Ok(rel) = canon_target.strip_prefix(&canon_root) {
            return Some(rel.to_path_buf());
        }
        if let Ok(rel) = canon_target.strip_prefix(root) {
            return Some(rel.to_path_buf());
        }
    }
    None
}

fn canonicalize_with_tail(path: &Path) -> Option<PathBuf> {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return Some(canon);
    }
    let mut cur = path;
    let mut tail = Vec::new();
    while let Some(name) = cur.file_name() {
        tail.push(name.to_os_string());
        cur = cur.parent()?;
        if let Ok(mut canon) = std::fs::canonicalize(cur) {
            for seg in tail.into_iter().rev() {
                canon.push(seg);
            }
            return Some(canon);
        }
    }
    None
}

fn from_posix(text: &str) -> String {
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        if is_safe(byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

fn to_posix_string(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    decode(rest)
}

/// Characters that never need escaping, per RFC 3986's `unreserved` set. `/`
/// is added because it is the separator we are building, and `:` because the
/// drive letter's colon is conventionally left bare and servers expect it.
fn is_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':')
}

/// Strip Windows' extended-length prefix.
///
/// `Path::canonicalize` hands back `\\?\C:\src\main.rs`. It names the same
/// file, but no language server accepts that spelling, and it is not what
/// comes back in a URI either — so a path canonicalised on the way in would
/// never match the diagnostics that came back for it. Both directions
/// normalise through here, which is what makes those two spellings one key.
pub fn plain(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        // `\\?\UNC\server\share` is the share `\\server\share`.
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

pub fn from_path(path: impl AsRef<Path>) -> String {
    let path = plain(path);
    let text = path.to_string_lossy().replace('\\', "/");

    let mut uri = String::from("file://");
    // A Windows path starts at a drive letter rather than at `/`, so the
    // authority-terminating slash has to be supplied.
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        if is_safe(byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

pub fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file:///c/x` and `file://localhost/c/x` both mean the local machine;
    // anything else is a host we cannot open.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);

    let decoded = decode(rest)?;
    // `/C:/src` is a Windows path wearing a URI's leading slash.
    let trimmed = match decoded.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => &decoded[1..],
        _ => decoded.as_str(),
    };
    Some(PathBuf::from(
        trimmed.replace('/', std::path::MAIN_SEPARATOR_STR),
    ))
}

fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_path_gets_three_slashes_and_a_bare_drive_colon() {
        assert_eq!(
            from_path(r"C:\src\proton\main.rs"),
            "file:///C:/src/proton/main.rs"
        );
    }

    #[test]
    fn a_unix_path_keeps_its_own_leading_slash() {
        assert_eq!(from_path("/home/r/main.rs"), "file:///home/r/main.rs");
    }

    #[test]
    fn spaces_and_other_awkwardness_are_percent_encoded() {
        let uri = from_path(r"C:\My Code\a+b\ünicode.rs");
        assert_eq!(uri, "file:///C:/My%20Code/a%2Bb/%C3%BCnicode.rs");
        // And survive the trip back, which is the only thing that matters.
        assert_eq!(
            to_path(&uri).unwrap(),
            PathBuf::from(format!(
                "C:{sep}My Code{sep}a+b{sep}ünicode.rs",
                sep = std::path::MAIN_SEPARATOR
            ))
        );
    }

    #[test]
    fn a_path_survives_the_round_trip() {
        for path in [r"C:\src\main.rs", r"D:\a b\c.rs"] {
            let back = to_path(&from_path(path)).unwrap();
            assert_eq!(
                back,
                PathBuf::from(path.replace('\\', std::path::MAIN_SEPARATOR_STR))
            );
        }
    }

    #[test]
    fn an_extended_length_path_is_spelled_the_ordinary_way() {
        // What `canonicalize` returns on Windows. Left alone, it produces a
        // URI no server recognises — and, worse, one that never matches the
        // diagnostics that come back.
        assert_eq!(
            from_path(r"\\?\C:\src\main.rs"),
            "file:///C:/src/main.rs",
            "the verbatim prefix must not reach the wire"
        );
        assert_eq!(plain(r"\\?\C:\src"), PathBuf::from(r"C:\src"));
        assert_eq!(
            plain(r"\\?\UNC\host\share\x.rs"),
            PathBuf::from(r"\\host\share\x.rs")
        );
        // An ordinary path is left exactly as it was.
        assert_eq!(plain(r"C:\src\main.rs"), PathBuf::from(r"C:\src\main.rs"));
    }

    #[test]
    fn localhost_is_this_machine_and_a_real_host_is_not_a_path() {
        assert!(to_path("file://localhost/home/r/x.rs").is_some());
        assert!(to_path("https://example.com/x.rs").is_none());
    }
}
