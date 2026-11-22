//! Bidirectional path & `file://` URI mapping between the host workspace
//! and the dev container's Linux workspace folder.

use std::path::{Component, Path, PathBuf};

/// Maps paths and `file://` URIs between the host workspace root and the
/// mounted workspace folder inside a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMap {
    pub host_root: PathBuf,
    pub container_root: String,
}

impl PathMap {
    pub fn new(host_root: impl Into<PathBuf>, container_root: impl Into<String>) -> Self {
        let mut container_root = container_root.into();
        if container_root.is_empty() {
            container_root = "/workspace".to_string();
        }
        while container_root.len() > 1 && container_root.ends_with('/') {
            container_root.pop();
        }
        Self {
            host_root: host_root.into(),
            container_root,
        }
    }

    /// Translate a host path inside the workspace into a container POSIX path.
    ///
    /// Handles macOS `/var` ↔ `/private/var` canonicalization mismatches by
    /// checking canonicalized forms of both `host_root` and `host_path` (or its
    /// deepest existing ancestor if the file does not exist yet).
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
        if out.is_empty() {
            Some("/".to_string())
        } else {
            Some(out)
        }
    }

    /// Translate a container POSIX path back to a host `PathBuf`.
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

    /// Translate a host `file://` URI into a container `file://` URI.
    pub fn host_uri_to_container_uri(&self, uri: &str) -> Option<String> {
        let host_path = uri_to_path(uri)?;
        let container_path = self.host_to_container(&host_path)?;
        Some(path_to_unix_uri(&container_path))
    }

    /// Translate a container `file://` URI into a host `file://` URI.
    pub fn container_uri_to_host_uri(&self, uri: &str) -> Option<String> {
        let container_path = uri_to_posix_string(uri)?;
        let host_path = self.container_to_host(&container_path)?;
        Some(host_path_to_uri(&host_path))
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

fn is_safe_uri_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':')
}

pub fn path_to_unix_uri(posix_path: &str) -> String {
    let mut uri = String::from("file://");
    if !posix_path.starts_with('/') {
        uri.push('/');
    }
    for b in posix_path.bytes() {
        if is_safe_uri_byte(b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

pub fn host_path_to_uri(path: &Path) -> String {
    let text = path.to_string_lossy();
    let plain = text
        .strip_prefix(r"\\?\UNC\")
        .map(|r| format!(r"\\{r}"))
        .or_else(|| text.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| text.into_owned());
    let slashed = plain.replace('\\', "/");
    path_to_unix_uri(&slashed)
}

fn uri_to_posix_string(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    percent_decode(rest)
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let decoded = uri_to_posix_string(uri)?;
    let trimmed = match decoded.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => &decoded[1..],
        _ => decoded.as_str(),
    };
    Some(PathBuf::from(
        trimmed.replace('/', std::path::MAIN_SEPARATOR_STR),
    ))
}

fn percent_decode(text: &str) -> Option<String> {
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
    use super::PathMap;
    use std::path::PathBuf;

    #[test]
    fn maps_root_and_nested_files_in_both_directions() {
        let map = PathMap::new("/Users/me/proj", "/workspaces/proj");
        assert_eq!(
            map.host_to_container(&PathBuf::from("/Users/me/proj")).as_deref(),
            Some("/workspaces/proj")
        );
        assert_eq!(
            map.host_to_container(&PathBuf::from("/Users/me/proj/src/main.rs"))
                .as_deref(),
            Some("/workspaces/proj/src/main.rs")
        );
        assert_eq!(
            map.container_to_host("/workspaces/proj"),
            Some(PathBuf::from("/Users/me/proj"))
        );
        assert_eq!(
            map.container_to_host("/workspaces/proj/src/main.rs"),
            Some(PathBuf::from("/Users/me/proj/src/main.rs"))
        );
    }

    #[test]
    fn rejects_paths_outside_workspace_or_sharing_prefix() {
        let map = PathMap::new("/Users/me/proj", "/workspaces/proj");
        assert_eq!(
            map.host_to_container(&PathBuf::from("/Users/me/proj-other/a.rs")),
            None
        );
        assert_eq!(
            map.container_to_host("/workspaces/proj-other/a.rs"),
            None
        );
        assert_eq!(
            map.container_to_host("/workspaces/proj/../etc/passwd"),
            None
        );
    }

    #[test]
    fn handles_spaces_unicode_and_file_uris() {
        let map = PathMap::new("/Users/me/My App", "/workspaces/My App");
        let host_uri = "file:///Users/me/My%20App/src/%C3%BCber%20file.rs";
        let cont_uri = map.host_uri_to_container_uri(host_uri).unwrap();
        assert_eq!(cont_uri, "file:///workspaces/My%20App/src/%C3%BCber%20file.rs");
        assert_eq!(map.container_uri_to_host_uri(&cont_uri).unwrap(), host_uri);
    }

    #[test]
    fn handles_canonicalized_macos_var_paths() {
        let raw_tmp = std::env::temp_dir().join(format!("proton-pathmap-{}", std::process::id()));
        std::fs::create_dir_all(raw_tmp.join("sub")).unwrap();
        let canon_tmp = std::fs::canonicalize(&raw_tmp).unwrap();

        let map = PathMap::new(&canon_tmp, "/workspaces/test");
        let nested_via_raw = raw_tmp.join("sub").join("new_file.rs");
        assert_eq!(
            map.host_to_container(&nested_via_raw).as_deref(),
            Some("/workspaces/test/sub/new_file.rs")
        );

        let _ = std::fs::remove_dir_all(&raw_tmp);
    }
}
