//! What the Artifact tool publishes, kept a version at a time.
//!
//! Claude Code's Artifact tool publishes a page to claude.ai and answers with
//! where it went: the URL, the artifact's id, its title and which version
//! this is. The page itself is a file on this machine, and files change —
//! the next draft overwrites the last — so at the moment it is published
//! Proton copies it, with any files it brought along, into a folder of its
//! own. That copy is what a notebook reads and shows: the version that was
//! published, offline, with no sign-in.
//!
//! The copies live under the user's data directory rather than in a project,
//! because claude.ai keeps one gallery per person, not per folder.
//!
//! The pane loads them from `artifact://`, a scheme the shell answers from
//! [`resolve`] alone — so the only files a page can ever be served from are
//! these copies, under a policy of the page's own ([`CSP`]) that is not the
//! editor's.

use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// The policy every artifact page is served with.
///
/// The CDNs are the ones claude.ai lets an artifact load from, so a page
/// that works there works here; inline script is allowed because artifacts
/// are written that way. What it does not allow is the one thing that
/// matters inside an editor: `ipc:` and the shell's own origin are absent
/// from every directive, so a page cannot reach Proton, only draw itself.
///
/// Pictures and media may come from anywhere — and from the page's own
/// folder, named outright: WebKit's `*` matches the web's schemes, not
/// `artifact:`, so a page's clips and posters were refused without it.
pub const CSP: &str = "default-src 'self' artifact: http://artifact.localhost data: blob:; \
script-src 'self' 'unsafe-inline' 'unsafe-eval' artifact: http://artifact.localhost blob: \
https://cdnjs.cloudflare.com https://cdn.jsdelivr.net https://unpkg.com https://cdn.tailwindcss.com https://code.jquery.com; \
style-src 'self' 'unsafe-inline' artifact: http://artifact.localhost https://fonts.googleapis.com \
https://cdnjs.cloudflare.com https://cdn.jsdelivr.net https://unpkg.com; \
font-src 'self' artifact: http://artifact.localhost data: https://fonts.gstatic.com https://cdnjs.cloudflare.com https://cdn.jsdelivr.net https://unpkg.com; \
img-src 'self' artifact: http://artifact.localhost * data: blob:; media-src 'self' artifact: http://artifact.localhost * data: blob:; \
connect-src 'self' artifact: http://artifact.localhost https://cdnjs.cloudflare.com https://cdn.jsdelivr.net https://unpkg.com; \
frame-src 'none'; object-src 'none'; base-uri 'self'; form-action 'none'";

/// One published version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Version {
    /// claude.ai's count: 1, 2, 3…
    pub seq: u32,
    /// The page within the version's folder.
    pub file: String,
    /// When it was published, in milliseconds since the epoch.
    #[ts(type = "number")]
    pub at: u64,
}

/// An artifact and every version of it this machine has published.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Artifact {
    /// claude.ai's id for it, a UUID.
    pub id: String,
    pub url: String,
    pub title: String,
    pub icon: String,
    pub description: String,
    /// The project and session that published it.
    pub project: String,
    pub session: String,
    pub versions: Vec<Version>,
}

/// What a transcript row needs to show an artifact it published.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct ArtifactRef {
    pub id: String,
    pub seq: u32,
    pub title: String,
    pub url: String,
    pub icon: String,
    /// The page within the version's folder; empty when nothing could be
    /// kept, which leaves claude.ai as the only place to see it.
    pub file: String,
}

/// Where the copies live: `PROTON_ARTIFACTS` if it is set, otherwise
/// Proton's folder in the user's data directory.
pub fn root() -> PathBuf {
    if let Some(dir) = std::env::var_os("PROTON_ARTIFACTS") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("dev.razeen.proton")
        .join("artifacts")
}

/// What an import brought in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Imported {
    /// Artifacts the store did not have.
    pub added: usize,
    /// New versions of ones it did.
    pub updated: usize,
    /// Of those, how many came without a page — theirs is gone from disk.
    pub remote: usize,
    /// Versions kept before without files their page uses — clips, images,
    /// scripts — given them now.
    pub mended: usize,
}

/// The gallery: every artifact kept, newest first, in `index.json`.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    items: Vec<Artifact>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// One segment of a path that is safe to join under a folder of ours: no
/// parent, no root, no prefix, nothing empty.
fn plain(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Where a publish's files are found, best first, as the Artifact tool finds
/// them: under its `root` when it named one, else the working directory it
/// was called in — which a page named by a relative path gives away, and
/// which is otherwise the project's — and, last, beside the page itself.
fn bases(input: &Value, page: Option<&Path>, project: &str) -> Vec<PathBuf> {
    let mut cwd: Vec<PathBuf> = Vec::new();
    if let (Some(named), Some(page)) = (input.get("file_path").and_then(Value::as_str), page) {
        let named: Vec<Component> = Path::new(named).components().filter(|c| !matches!(c, Component::CurDir)).collect();
        let whole: Vec<Component> = page.components().collect();
        let relative = named.iter().all(|c| matches!(c, Component::Normal(_)));
        if relative && !named.is_empty() && whole.len() > named.len() && whole[whole.len() - named.len()..] == named[..] {
            cwd.push(whole[..whole.len() - named.len()].iter().collect());
        }
    }
    if !project.is_empty() {
        cwd.push(PathBuf::from(project));
    }
    let mut out: Vec<PathBuf> = Vec::new();
    if let Some(root) = input.get("root").and_then(Value::as_str).filter(|r| !r.is_empty()) {
        match Path::new(root).is_absolute() {
            true => out.push(PathBuf::from(root)),
            false => out.extend(cwd.iter().map(|dir| dir.join(root))),
        }
    }
    out.extend(cwd);
    if let Some(dir) = page.and_then(Path::parent) {
        out.push(dir.to_path_buf());
    }
    let mut seen: Vec<PathBuf> = Vec::new();
    out.retain(|dir| {
        let new = !seen.contains(dir);
        seen.push(dir.clone());
        new
    });
    out
}

/// What a publish brought with it: each file's published path and where it
/// came from, as the tool takes them — a map of published path to source
/// (a path, or `{from}`), or a list of paths each published as itself.
/// One removed (`null`), or copied from another artifact, has no source here.
fn brought(files: Option<&Value>) -> Vec<(PathBuf, String)> {
    match files {
        Some(Value::Object(map)) => map
            .iter()
            .filter_map(|(published, from)| {
                let from = match from {
                    Value::String(from) => from.as_str(),
                    Value::Object(spec) => spec.get("from").and_then(Value::as_str)?,
                    _ => return None,
                };
                Some((plain(published)?, from.to_string()))
            })
            .collect(),
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(|item| {
                let path = item.as_str().or_else(|| item.get("path").and_then(Value::as_str))?;
                Some((plain(path)?, path.to_string()))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The paths a publish removed from its artifact: `null` in its map.
fn removed(files: Option<&Value>) -> Vec<PathBuf> {
    files.and_then(Value::as_object).into_iter().flatten().filter(|(_, from)| from.is_null()).filter_map(|(published, _)| plain(published)).collect()
}

/// Every file under a folder, as paths within it.
fn within(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for entry in std::fs::read_dir(dir.join(&rel)).into_iter().flatten().flatten() {
            let rel = rel.join(entry.file_name());
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(rel),
                Ok(t) if t.is_file() => out.push(rel),
                _ => {}
            }
        }
    }
    out
}

/// A file put in place: linked where it can be — a version's clips are
/// the same bytes as the last's — and copied where it cannot.
fn place(from: &Path, to: &Path) -> bool {
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(to);
    std::fs::hard_link(from, to).is_ok() || std::fs::copy(from, to).is_ok()
}

/// A UUID, or anything else made of the characters one is: an id safe to
/// make a folder of.
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

impl Store {
    pub fn open() -> Store {
        Store::at(root())
    }

    pub fn at(root: PathBuf) -> Store {
        let items = std::fs::read_to_string(root.join("index.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Store { root, items }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list(&self) -> &[Artifact] {
        &self.items
    }

    fn save(&self) {
        // Losing the index costs the gallery its list, not the copies.
        let _ = std::fs::create_dir_all(&self.root);
        if let Ok(text) = serde_json::to_string_pretty(&self.items) {
            let temp = self.root.join("index.json.tmp");
            if std::fs::write(&temp, text + "\n").is_ok() {
                let _ = std::fs::rename(&temp, self.root.join("index.json"));
            }
        }
    }

    /// Keep what one call of the Artifact tool published.
    ///
    /// `input` is what the tool was called with and `result` what it answered
    /// — its structured result, which carries the URL, the artifact's id, the
    /// version number and the path of the page it published. Anything that is
    /// not a published page — a listing, a read, a quickstart — has no id
    /// and no version, and is not kept.
    pub fn record(&mut self, input: &Value, result: &Value, project: &str, session: &str) -> Option<ArtifactRef> {
        let id = result.get("artifact_id").and_then(Value::as_str)?.to_string();
        let seq = result.get("seq").and_then(Value::as_u64)? as u32;
        let url = result.get("url").and_then(Value::as_str).unwrap_or_default().to_string();
        if !safe_id(&id) {
            return None;
        }
        let title = result
            .get("title")
            .and_then(Value::as_str)
            .or_else(|| input.get("title").and_then(Value::as_str))
            .unwrap_or("Artifact")
            .to_string();
        let source = result
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| input.get("file_path").and_then(Value::as_str))
            .map(PathBuf::from);

        // An artifact already kept — imported, say, before it was published
        // again from here — is the same one, whatever id it was kept under.
        let id = match self.items.iter().find(|a| a.id == id || crate::import::same_url(&a.url, &url)) {
            Some(kept) => kept.id.clone(),
            None => id,
        };
        let bases = bases(input, source.as_deref(), project);
        let file = self.keep(&id, seq, source.as_deref(), input.get("files"), &bases);

        let icon = result
            .get("icon")
            .and_then(Value::as_str)
            .or_else(|| input.get("icon").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        let description = input.get("description").and_then(Value::as_str).unwrap_or_default().to_string();
        let version = Version { seq, file: file.clone(), at: now() };
        match self.items.iter().position(|a| a.id == id) {
            Some(at) => {
                let mut artifact = self.items.remove(at);
                artifact.title = title.clone();
                if !url.is_empty() {
                    artifact.url = url.clone();
                }
                if !icon.is_empty() {
                    artifact.icon = icon.clone();
                }
                if !description.is_empty() {
                    artifact.description = description;
                }
                artifact.versions.retain(|v| v.seq != seq);
                artifact.versions.push(version);
                artifact.versions.sort_by_key(|v| v.seq);
                self.items.insert(0, artifact);
            }
            None => self.items.insert(
                0,
                Artifact {
                    id: id.clone(),
                    url: url.clone(),
                    title: title.clone(),
                    icon: icon.clone(),
                    description,
                    project: project.to_string(),
                    session: session.to_string(),
                    versions: vec![version],
                },
            ),
        }
        self.save();
        Some(ArtifactRef { id, seq, title, url, icon, file })
    }

    /// Copy a page — and the files it brought with it, found under `bases`
    /// — into a version's folder. A version is what claude.ai serves after
    /// that publish: the files the last version had, unless this one
    /// replaced or removed them, and the ones it brought. The page's name in
    /// the folder, or nothing when there was no page to copy.
    fn keep(&self, id: &str, seq: u32, source: Option<&Path>, files: Option<&Value>, bases: &[PathBuf]) -> String {
        let Some(source) = source.filter(|path| path.is_file()) else { return String::new() };
        let folder = self.root.join(id).join(seq.to_string());
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "index.html".into());
        if std::fs::create_dir_all(&folder).is_err() || std::fs::copy(source, folder.join(&name)).is_err() {
            return String::new();
        }
        // What the last version had, carried over.
        let earlier = self.items.iter().find(|a| a.id == id).and_then(|a| a.versions.iter().filter(|v| v.seq < seq && !v.file.is_empty()).max_by_key(|v| v.seq).cloned());
        if let Some(earlier) = earlier {
            let from = self.root.join(id).join(earlier.seq.to_string());
            let gone = removed(files);
            for rel in within(&from) {
                if rel == Path::new(&earlier.file) || rel == Path::new(&name) || gone.contains(&rel) || folder.join(&rel).exists() {
                    continue;
                }
                place(&from.join(&rel), &folder.join(&rel));
            }
        }
        self.bring(&folder, files, bases, false);
        name
    }

    /// Put the files a publish brought into a version's folder, each found
    /// under the first of `bases` that has it. Only those missing, when
    /// `missing`; how many were put.
    fn bring(&self, folder: &Path, files: Option<&Value>, bases: &[PathBuf], missing: bool) -> usize {
        let mut put = 0;
        for (to, from) in brought(files) {
            let to = folder.join(to);
            if missing && to.is_file() {
                continue;
            }
            let from = Path::new(&from);
            let found = match from.is_absolute() {
                true => Some(from.to_path_buf()).filter(|f| f.is_file()),
                false => bases.iter().map(|base| base.join(from)).find(|f| f.is_file()),
            };
            // Copied, never linked: the source is the person's, and may be
            // written over in place.
            let Some(found) = found else { continue };
            if let Some(parent) = to.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::copy(&found, &to).is_ok() {
                put += 1;
            }
        }
        put
    }

    /// Bring in artifacts published before Proton kept them.
    ///
    /// One new to the store is added with the page as it is now; one already
    /// kept gets a new version only when its page has changed since. One
    /// whose page is gone is kept as its address, to open on claude.ai.
    pub fn import(&mut self, found: Vec<crate::import::Found>) -> Imported {
        let mut done = Imported::default();
        for one in found {
            let id = one
                .id
                .clone()
                .filter(|id| safe_id(id))
                .unwrap_or_else(|| crate::import::id_from_url(&one.url));
            if !safe_id(&id) {
                continue;
            }
            let at = self.items.iter().position(|a| a.id == id || crate::import::same_url(&a.url, &one.url));
            let key = at.map(|i| self.items[i].id.clone()).unwrap_or_else(|| id.clone());
            let source = one.source.clone().filter(|p| p.is_file());
            let input = serde_json::json!({ "files": one.files.clone().unwrap_or(Value::Null) });
            let bases = bases(&input, source.as_deref(), &one.project);
            let latest = at.and_then(|i| self.items[i].versions.last().cloned());
            if let Some(latest) = &latest {
                let kept = self.root.join(&key).join(latest.seq.to_string()).join(&latest.file);
                let unchanged = match &source {
                    Some(source) => !latest.file.is_empty() && std::fs::read(source).ok() == std::fs::read(&kept).ok(),
                    None => true,
                };
                if unchanged {
                    // Kept before its files were found where it found them:
                    // given them now.
                    if !latest.file.is_empty() && self.bring(&self.root.join(&key).join(latest.seq.to_string()), one.files.as_ref(), &bases, true) > 0 {
                        done.mended += 1;
                    }
                    continue;
                }
            }
            let seq = match (one.seq, &latest) {
                (Some(seq), Some(latest)) if seq > latest.seq => seq,
                (_, Some(latest)) => latest.seq + 1,
                (Some(seq), None) => seq.max(1),
                (None, None) => 1,
            };
            let file = self.keep(&key, seq, source.as_deref(), one.files.as_ref(), &bases);
            if file.is_empty() {
                done.remote += 1;
            }
            let version = Version { seq, file, at: if one.at > 0 { one.at } else { now() } };
            match at {
                Some(i) => {
                    let artifact = &mut self.items[i];
                    if !one.title.is_empty() {
                        artifact.title = one.title.clone();
                    }
                    artifact.versions.push(version);
                    artifact.versions.sort_by_key(|v| v.seq);
                    done.updated += 1;
                }
                None => {
                    self.items.push(Artifact {
                        id: key,
                        url: one.url.clone(),
                        title: if one.title.is_empty() { "Artifact".into() } else { one.title.clone() },
                        icon: one.icon.clone(),
                        description: one.description.clone(),
                        project: one.project.clone(),
                        session: one.session.clone(),
                        versions: vec![version],
                    });
                    done.added += 1;
                }
            }
        }
        // Newest first, by when each was last published.
        self.items.sort_by(|a, b| {
            let last = |x: &Artifact| x.versions.last().map_or(0, |v| v.at);
            last(b).cmp(&last(a))
        });
        self.save();
        done
    }

    /// Forget one, and the copies with it.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|a| a.id != id);
        if self.items.len() == before {
            return false;
        }
        if safe_id(id) {
            let _ = std::fs::remove_dir_all(self.root.join(id));
        }
        self.save();
        true
    }
}

/// The file an `artifact://` path names: `/<id>/<seq>/<path within>`, and
/// only ever a file inside `root`. Anything else — a parent directory, an
/// absolute path, an id that is not one — is nothing.
pub fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let decoded = decode(path)?;
    let mut parts = decoded.trim_start_matches('/').splitn(3, '/');
    let id = parts.next()?;
    let seq = parts.next()?;
    let rest = parts.next().filter(|r| !r.is_empty()).unwrap_or("index.html");
    if !safe_id(id) || seq.is_empty() || !seq.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let file = root.join(id).join(seq).join(plain(rest)?);
    file.is_file().then_some(file)
}

/// Percent-decoding, for a path that came through a URL. Malformed escapes
/// and decoded text that is not UTF-8 are refused rather than guessed at.
fn decode(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What a file is, by its extension, for the `Content-Type` it is served with.
pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("md" | "markdown") => "text/markdown; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("mp4" | "m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        Some("ogv") => "video/ogg",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("flac") => "audio/flac",
        Some("ogg" | "oga" | "opus") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("vtt") => "text/vtt; charset=utf-8",
        Some("xml") => "application/xml",
        Some("tsv") => "text/tab-separated-values; charset=utf-8",
        Some("map" | "geojson") => "application/json",
        Some("glb") => "model/gltf-binary",
        Some("gltf") => "model/gltf+json",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain; charset=utf-8",
        Some("csv") => "text/csv; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// The bytes asked for by a `Range` header, of a file so long: `[start, end]`.
/// A player asks for one range at a time; more than one is answered as the
/// first.
pub fn range(header: Option<&str>, len: u64) -> Option<(u64, u64)> {
    let spec = header?.trim().strip_prefix("bytes=")?;
    let (from, to) = spec.split(',').next()?.trim().split_once('-')?;
    if len == 0 {
        return None;
    }
    let (start, end) = match (from.trim(), to.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n.max(1)), len - 1)
        }
        (start, "") => (start.parse().ok()?, len - 1),
        (start, end) => (start.parse().ok()?, end.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len).then_some((start, end))
}

/// The most of a file answered at once when a range is asked: a player
/// asks again for the rest.
pub const PIECE: u64 = 4 * 1024 * 1024;

/// An answer to a request for a file: its status, headers and bytes.
pub struct Served {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

/// A file answered as a player asks for it: whole, or — for `<video>` and
/// `<audio>`, which seek by asking for bytes — the range asked, a piece at
/// a time, 206 with where it is. A range the file has not got is 416.
pub fn answer(file: &Path, kind: &str, range_header: Option<&str>) -> Option<Served> {
    use std::io::{Read, Seek, SeekFrom};
    let mut handle = std::fs::File::open(file).ok()?;
    let len = handle.metadata().ok()?.len();
    let mut headers = vec![("Content-Type", kind.to_string()), ("Accept-Ranges", "bytes".to_string())];
    let asked = range(range_header, len);
    if range_header.is_some() && asked.is_none() {
        headers.push(("Content-Range", format!("bytes */{len}")));
        return Some(Served { status: 416, headers, body: Vec::new() });
    }
    let (start, end) = asked.map_or((0, len.saturating_sub(1)), |(s, e)| (s, e.min(s + PIECE - 1)));
    let mut body = vec![0u8; if len == 0 { 0 } else { (end - start + 1) as usize }];
    handle.seek(SeekFrom::Start(start)).ok()?;
    handle.read_exact(&mut body).ok()?;
    if asked.is_some() {
        headers.push(("Content-Range", format!("bytes {start}-{end}/{len}")));
    }
    Some(Served { status: if asked.is_some() { 206 } else { 200 }, headers, body })
}

/// Answer `artifact://localhost/<path>`: a kept file, under the artifacts'
/// policy, whole or the range a player asked for; or 404.
pub fn serve(root: &Path, path: &str, range_header: Option<&str>) -> Served {
    let common = || {
        vec![
            ("Content-Security-Policy", CSP.to_string()),
            ("X-Content-Type-Options", "nosniff".to_string()),
            // A sandboxed frame's origin is opaque, so a page fetching its
            // own data file is a cross-origin request to itself.
            ("Access-Control-Allow-Origin", "*".to_string()),
        ]
    };
    match resolve(root, path).and_then(|file| answer(&file, content_type(&file), range_header)) {
        Some(mut served) => {
            served.headers.extend(common());
            served
        }
        None => {
            let mut headers = common();
            headers.push(("Content-Type", "text/plain; charset=utf-8".to_string()));
            Served { status: 404, headers, body: b"This artifact is not kept here.".to_vec() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-artifacts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_published_page_is_kept_as_it_was_and_versions_stack() {
        let dir = scratch("keep");
        let page = dir.join("draft.html");
        std::fs::write(&page, "<title>One</title>").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/app.js"), "1").unwrap();
        let mut store = Store::at(dir.join("store"));
        let input = json!({ "file_path": page, "description": "A page", "files": { "js/app.js": "assets/app.js" } });
        let result = json!({ "url": "https://claude.ai/artifact/abc", "path": page,
            "artifact_id": "e7b920f4-6430-4bdf-9b66-42d50ace6ffd", "title": "One", "seq": 1, "icon": "chart" });
        let kept = store.record(&input, &result, "/p", "s").expect("kept");
        assert_eq!((kept.seq, kept.file.as_str(), kept.icon.as_str()), (1, "draft.html", "chart"));

        // The draft changes; version one does not.
        std::fs::write(&page, "<title>Two</title>").unwrap();
        let result = json!({ "url": "https://claude.ai/artifact/abc", "path": page,
            "artifact_id": "e7b920f4-6430-4bdf-9b66-42d50ace6ffd", "title": "Two", "seq": 2 });
        store.record(&json!({ "file_path": page }), &result, "/p", "s").unwrap();

        let root = store.root().to_path_buf();
        let one = resolve(&root, "/e7b920f4-6430-4bdf-9b66-42d50ace6ffd/1/draft.html").unwrap();
        let two = resolve(&root, "/e7b920f4-6430-4bdf-9b66-42d50ace6ffd/2/draft.html").unwrap();
        assert_eq!(std::fs::read_to_string(one).unwrap(), "<title>One</title>");
        assert_eq!(std::fs::read_to_string(two).unwrap(), "<title>Two</title>");
        assert!(resolve(&root, "/e7b920f4-6430-4bdf-9b66-42d50ace6ffd/1/js/app.js").is_some());

        // One artifact, two versions, the title the latest's — and it survives a reopen.
        let reopened = Store::at(root);
        assert_eq!(reopened.list().len(), 1);
        assert_eq!(reopened.list()[0].title, "Two");
        assert_eq!(reopened.list()[0].versions.iter().map(|v| v.seq).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(reopened.list()[0].description, "A page");
    }

    #[test]
    fn importing_adds_what_is_new_versions_what_changed_and_keeps_the_rest_as_addresses() {
        use crate::import::Found;
        let dir = scratch("import");
        let page = dir.join("chart.html");
        std::fs::write(&page, "v1").unwrap();
        let mut store = Store::at(dir.join("store"));
        let found = |source: Option<PathBuf>, at: u64| Found {
            url: "https://claude.ai/artifact/Chart9".into(), id: None, title: "Chart".into(), source,
            files: None, icon: "chart".into(), description: String::new(), project: "/p".into(), session: "s".into(), seq: None, at,
        };
        let gone = Found { url: "https://claude.ai/artifact/Gone1".into(), source: Some(dir.join("missing.html")), ..found(None, 5) };

        let done = store.import(vec![found(Some(page.clone()), 10), gone.clone()]);
        assert_eq!(done, Imported { added: 2, updated: 0, remote: 1, mended: 0 });
        // Again, unchanged: nothing new.
        assert_eq!(store.import(vec![found(Some(page.clone()), 10), gone]), Imported::default());
        // The page changed on disk: a second version.
        std::fs::write(&page, "v2").unwrap();
        assert_eq!(store.import(vec![found(Some(page.clone()), 20)]).updated, 1);
        let chart = store.list().iter().find(|a| a.id == "Chart9").unwrap();
        assert_eq!(chart.versions.iter().map(|v| v.seq).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(std::fs::read_to_string(resolve(store.root(), "/Chart9/1/chart.html").unwrap()).unwrap(), "v1");
        assert_eq!(store.list().iter().find(|a| a.id == "Gone1").unwrap().versions[0].file, "");

        // Published again from here, it joins the imported one rather than doubling it.
        let result = json!({ "url": "https://claude.ai/artifact/Chart9", "path": page, "artifact_id": "0f8fad5b-d9cb-469f-a165-70867728950e", "title": "Chart", "seq": 7 });
        let kept = store.record(&json!({ "file_path": page }), &result, "/p", "s").unwrap();
        assert_eq!((kept.id.as_str(), kept.seq), ("Chart9", 7));
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn what_is_not_a_published_page_is_not_kept() {
        let dir = scratch("skip");
        let mut store = Store::at(dir.join("store"));
        let quickstart = json!({ "quickstart": { "intent": "other" } });
        assert!(store.record(&json!({ "action": "quickstart" }), &quickstart, "/p", "s").is_none());
        let sneaky = json!({ "artifact_id": "../../etc", "seq": 1, "url": "x" });
        assert!(store.record(&json!({}), &sneaky, "/p", "s").is_none());
        assert!(store.list().is_empty());
    }

    #[test]
    fn only_files_inside_the_store_can_be_served() {
        let dir = scratch("serve");
        let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
        std::fs::create_dir_all(dir.join(id).join("3")).unwrap();
        std::fs::write(dir.join(id).join("3").join("index.html"), "ok").unwrap();
        std::fs::write(dir.join("secret.txt"), "no").unwrap();
        assert!(resolve(&dir, &format!("/{id}/3/index.html")).is_some());
        assert!(resolve(&dir, &format!("/{id}/3/")).is_some(), "a folder serves its index");
        assert!(resolve(&dir, &format!("/{id}/3/index.html?x=1#y")).is_some());
        for bad in [
            format!("/{id}/3/../../secret.txt"),
            format!("/{id}/3/%2e%2e/%2e%2e/secret.txt"),
            "/../secret.txt".to_string(),
            format!("/{id}/x/index.html"),
            format!("/{id}/3//etc/passwd"),
            format!("/{id}/3/%zz"),
        ] {
            assert!(resolve(&dir, &bad).is_none(), "{bad} was served");
        }
    }

    #[test]
    fn a_publishs_files_are_found_where_the_tool_found_them() {
        let cwd = scratch("bases");
        std::fs::create_dir_all(cwd.join("dev/build")).unwrap();
        std::fs::create_dir_all(cwd.join("media/clips")).unwrap();
        std::fs::create_dir_all(cwd.join("dev/out")).unwrap();
        std::fs::create_dir_all(cwd.join("data")).unwrap();
        let page = cwd.join("dev/build/artifact.html");
        std::fs::write(&page, "<video src=media/clips/ch1.mp4>").unwrap();
        std::fs::write(cwd.join("media/clips/ch1.mp4"), "clip").unwrap();
        std::fs::write(cwd.join("dev/out/app.js"), "js").unwrap();
        std::fs::write(cwd.join("data/x.json"), "{}").unwrap();
        let mut store = Store::at(cwd.join("store"));
        let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
        let result = |seq: u32| json!({ "url": "https://claude.ai/artifact/Clips", "path": page, "artifact_id": id, "title": "Clips", "seq": seq });
        // The page named relatively: the working directory is what is left.
        let input = json!({ "file_path": "dev/build/artifact.html", "files": { "media/clips/ch1.mp4": "media/clips/ch1.mp4" } });
        store.record(&input, &result(1), "", "s").unwrap();
        assert!(resolve(store.root(), &format!("/{id}/1/media/clips/ch1.mp4")).is_some(), "a clip found under the working directory");
        // Named absolutely, the project is where they are; a `root` is under it;
        // a list publishes each path as itself.
        let input = json!({ "file_path": page, "root": "dev/out", "files": { "js/app.js": "app.js" } });
        store.record(&input, &result(2), cwd.to_str().unwrap(), "s").unwrap();
        assert!(resolve(store.root(), &format!("/{id}/2/js/app.js")).is_some(), "found under the root it named");
        let input = json!({ "file_path": page, "files": [{ "path": "data/x.json" }] });
        store.record(&input, &result(3), cwd.to_str().unwrap(), "s").unwrap();
        assert!(resolve(store.root(), &format!("/{id}/3/data/x.json")).is_some(), "a list of paths");
        // Each version is what claude.ai serves after it: what the last had,
        // with what this one brought.
        assert!(resolve(store.root(), &format!("/{id}/3/media/clips/ch1.mp4")).is_some() && resolve(store.root(), &format!("/{id}/3/js/app.js")).is_some());
        // One removed is not carried on.
        store.record(&json!({ "file_path": page, "files": { "js/app.js": null } }), &result(4), cwd.to_str().unwrap(), "s").unwrap();
        assert!(resolve(store.root(), &format!("/{id}/4/js/app.js")).is_none() && resolve(store.root(), &format!("/{id}/4/data/x.json")).is_some());
    }

    #[test]
    fn importing_gives_a_version_kept_without_its_files_the_files_it_names() {
        use crate::import::Found;
        let cwd = scratch("mend");
        std::fs::create_dir_all(cwd.join("dev/build")).unwrap();
        std::fs::create_dir_all(cwd.join("media/clips")).unwrap();
        let page = cwd.join("dev/build/artifact.html");
        std::fs::write(&page, "<video>").unwrap();
        std::fs::write(cwd.join("media/clips/ch1.mp4"), "clip").unwrap();
        let mut store = Store::at(cwd.join("store"));
        let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
        // Kept as it was before this was fixed: the page alone.
        let result = json!({ "url": "https://claude.ai/artifact/Mend1", "path": page, "artifact_id": id, "title": "Mend", "seq": 1 });
        store.record(&json!({ "file_path": page }), &result, "", "s").unwrap();
        assert!(resolve(store.root(), &format!("/{id}/1/media/clips/ch1.mp4")).is_none());
        let found = Found {
            url: "https://claude.ai/artifact/Mend1".into(), id: Some(id.into()), title: "Mend".into(), source: Some(page.clone()),
            files: Some(json!({ "media/clips/ch1.mp4": "media/clips/ch1.mp4" })), icon: String::new(), description: String::new(),
            project: cwd.to_str().unwrap().into(), session: "s".into(), seq: Some(1), at: 1,
        };
        assert_eq!(store.import(vec![found.clone()]), Imported { mended: 1, ..Imported::default() });
        assert!(resolve(store.root(), &format!("/{id}/1/media/clips/ch1.mp4")).is_some(), "the clip it names, given it");
        assert_eq!(store.import(vec![found]), Imported::default(), "and only once");
    }

    #[test]
    fn a_player_is_answered_the_range_it_asks_for() {
        assert_eq!(range(Some("bytes=0-"), 100), Some((0, 99)));
        assert_eq!(range(Some("bytes=90-500"), 100), Some((90, 99)));
        assert_eq!(range(Some("bytes=-30"), 100), Some((70, 99)));
        assert_eq!(range(Some("bytes=100-"), 100), None);
        let dir = scratch("range");
        let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
        std::fs::create_dir_all(dir.join(id).join("1/media")).unwrap();
        std::fs::write(dir.join(id).join("1/media/clip.mp4"), (0..=255u8).cycle().take(10_000).collect::<Vec<_>>()).unwrap();
        let path = format!("/{id}/1/media/clip.mp4");
        let has = |served: &Served, name: &str, value: &str| served.headers.iter().any(|(k, v)| *k == name && v == value);
        let whole = serve(&dir, &path, None);
        assert_eq!((whole.status, whole.body.len()), (200, 10_000));
        assert!(has(&whole, "Content-Type", "video/mp4") && has(&whole, "Accept-Ranges", "bytes") && has(&whole, "Content-Security-Policy", CSP));
        let part = serve(&dir, &path, Some("bytes=256-511"));
        assert_eq!((part.status, part.body.len(), part.body[0]), (206, 256, 0));
        assert!(has(&part, "Content-Range", "bytes 256-511/10000"));
        assert_eq!(serve(&dir, &path, Some("bytes=20000-")).status, 416);
        let missing = serve(&dir, &format!("/{id}/1/nothing.mp4"), None);
        assert!(missing.status == 404 && has(&missing, "Content-Security-Policy", CSP));
        assert_eq!(content_type(Path::new("lecture.MOV")), "video/quicktime");
        assert_eq!(content_type(Path::new("talk.vtt")), "text/vtt; charset=utf-8");
    }

    #[test]
    fn the_page_policy_keeps_the_editor_out_of_reach() {
        // A page's own clips and pictures, named by its scheme: `*` is not it.
        for directive in ["img-src", "media-src"] {
            let rule = CSP.split(';').map(str::trim).find(|d| d.starts_with(directive)).unwrap();
            assert!(rule.contains(" artifact: ") && rule.contains("http://artifact.localhost"), "{rule}");
        }
        assert!(!CSP.contains("ipc:"));
        assert!(!CSP.contains("tauri:"));
        assert!(CSP.contains("script-src 'self' 'unsafe-inline'"));
        assert_eq!(content_type(Path::new("a/B.HTML")), "text/html; charset=utf-8");
        assert_eq!(content_type(Path::new("x.woff2")), "font/woff2");
    }
}
