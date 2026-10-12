//! A folder read as one text: a lab — its README, its code, its Makefile and
//! whatever else in it is text — each file under a line naming it,
//! `==> lab1/main.c <==`, as `head` heads several files at once. Read so, a
//! notebook holds a lab as one source, the tutor reads it as the lab it is,
//! and a citation names the file as well as its lines:
//! `[[S07 § lab1/main.c L12-40]]` ([`crate::chunk::folder`]).
//!
//! What a folder holds that is not text — a compiled program, a picture, a
//! Word document — is left out, and listed at the top with why, so nothing is
//! missing without a reason. Hidden files and links are passed over and the
//! rules' folders left out (`build/`, `node_modules/`, the person's own), as
//! the knowledge base's walk does; a file in iCloud is never brought down.

use std::path::{Path, PathBuf};

use crate::dataless;
use crate::extract::{self, Body, Pdfs, Unread};
use crate::kind::Kind;
use crate::rules::{RootRules, Rules};

/// The most files one folder's text is made of.
pub const MOST_FILES: usize = 400;
/// The most text it keeps, in bytes.
pub const MOST_TEXT: usize = 6 << 20;
/// The most files and folders looked at: a folder bigger than this is not
/// one lab, and one source cannot hold it.
pub const MOST_WALKED: usize = 5_000;
/// How many of what was left out are named, before the rest are counted.
const NAMED: usize = 40;

/// The line a file's text follows: `==> lab1/main.c <==`.
pub fn header(rel: &str) -> String {
    format!("==> {rel} <==")
}

/// The file a header line names.
pub fn headed(line: &str) -> Option<&str> {
    line.strip_prefix("==> ")?.strip_suffix(" <==").filter(|rel| !rel.is_empty())
}

/// What a walk of a folder passes over: the rules of the root it is in, and
/// where in that root it is.
#[derive(Debug, Clone)]
pub struct Scope {
    rules: RootRules,
    /// The folder's path under its root, `COMP2310/labs`; empty for a folder
    /// of no root.
    prefix: String,
}

impl Scope {
    /// A folder of a knowledge base's root labelled `label`, at `rel` under it.
    pub fn within(rules: &Rules, label: &str, rel: &str) -> Scope {
        Scope { rules: rules.under(label), prefix: rel.trim_matches('/').to_string() }
    }

    /// A folder anywhere else: the defaults alone.
    pub fn anywhere() -> Scope {
        Scope { rules: Rules::default().under(""), prefix: String::new() }
    }

    /// Why the folder itself is left out, if it is: by a rule of the root,
    /// or a folder above it.
    pub fn refused(&self) -> Option<String> {
        if self.prefix.is_empty() {
            return self.rules.whole().map(|why| why.say());
        }
        self.rules.why_anywhere(&self.prefix, true).map(|why| why.say())
    }

    fn why(&self, rel: &str, dir: bool) -> Option<String> {
        let full = match self.prefix.as_str() {
            "" => rel.to_string(),
            prefix => format!("{prefix}/{rel}"),
        };
        self.rules.why(&full, dir).map(|why| why.say())
    }
}

/// What a folder is, without reading it: how many files, how much, and the
/// newest change — what tells that it has changed since it was read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stamp {
    pub files: usize,
    pub size: u64,
    pub mtime: u64,
}

/// A folder, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub text: String,
    /// How many of its files are in the text.
    pub files: usize,
    pub stamp: Stamp,
}

/// A file a walk found.
struct Found {
    rel: String,
    path: PathBuf,
    size: u64,
    dataless: bool,
}

/// Every file under `dir` the scope lets in, a folder's README first, then
/// its files, then its folders, each by name; what was left out, with why;
/// and the stamp of all of it.
fn walk(dir: &Path, scope: &Scope) -> Result<(Vec<Found>, Vec<(String, String)>, Stamp), String> {
    let mut found = Vec::new();
    let mut left = Vec::new();
    let mut stamp = Stamp::default();
    let mut walked = 0;
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((at, rel)) = pending.pop() {
        let entries = std::fs::read_dir(&at).map_err(|e| format!("{}: {e}", at.display()))?;
        let mut files = Vec::new();
        let mut folders = Vec::new();
        for entry in entries.flatten() {
            walked += 1;
            if walked > MOST_WALKED {
                return Err(format!("it holds more than {MOST_WALKED} files and folders, more than one source can: add a folder inside it"));
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            // Hidden: `.git`, `.vscode`, `.DS_Store` — nobody's to read.
            if name.starts_with('.') {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
            // A link is never followed: it can lead anywhere.
            if meta.file_type().is_symlink() {
                continue;
            }
            let sub = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if meta.is_dir() {
                match scope.why(&sub, true) {
                    Some(why) => left.push((format!("{sub}/"), why)),
                    None => folders.push((entry.path(), sub)),
                }
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            if let Some(why) = scope.why(&sub, false) {
                left.push((sub, why));
                continue;
            }
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            stamp.files += 1;
            stamp.size += meta.len();
            stamp.mtime = stamp.mtime.max(mtime);
            files.push(Found { rel: sub, path: entry.path(), size: meta.len(), dataless: dataless::is_dataless(&meta) });
        }
        // What a lab is about comes first: its README, then the rest by name.
        let key = |rel: &str| {
            let name = rel.rsplit('/').next().unwrap_or(rel).to_lowercase();
            (!name.starts_with("readme"), name)
        };
        files.sort_by_key(|f| key(&f.rel));
        folders.sort_by_key(|(_, sub)| sub.to_lowercase());
        found.extend(files);
        // Taken from the end: pushed last first, so they are walked in order
        // — this folder's files, then each of its folders whole.
        let at_end = pending.len();
        for folder in folders {
            pending.insert(at_end, folder);
        }
    }
    Ok((found, left, stamp))
}

/// A folder's stamp: what [`read`] would record of it, without reading it.
pub fn stamp(dir: &Path, scope: &Scope) -> Result<Stamp, String> {
    walk(dir, scope).map(|(_, _, stamp)| stamp)
}

/// What a file of no kind Proton knows says, if it is text: a Makefile, a
/// `.json`, a `.yml`. Not if it holds a NUL, as a program or an archive
/// does, or is mostly not UTF-8.
fn sniff(path: &Path, size: u64) -> Result<String, String> {
    if size > Kind::Other.largest() {
        return Err(format!("too large to read, {} KB", size >> 10));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Err("not text".into());
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            let lossy = String::from_utf8_lossy(err.as_bytes()).into_owned();
            let bad = lossy.chars().filter(|c| *c == char::REPLACEMENT_CHARACTER).count();
            if bad * 20 > lossy.chars().count().max(1) {
                return Err("not text".into());
            }
            lossy
        }
    };
    match text.trim().is_empty() {
        true => Err("empty".into()),
        false => Ok(text),
    }
}

/// One file's words, as lines: a PDF's pages each under `--- page 3 ---`.
fn words(found: &Found, pdfs: &Pdfs) -> Result<String, String> {
    let kind = Kind::of(&found.path);
    let body = match kind {
        Kind::Markdown | Kind::Text | Kind::Code | Kind::Tex | Kind::Table | Kind::Notebook | Kind::Pdf | Kind::Captions => extract::read(&found.path, kind, pdfs),
        Kind::Document => return Err("a document: add it on its own to read it".into()),
        Kind::Slides => return Err("slides: add them on their own to read them".into()),
        Kind::Image => return Err("a picture".into()),
        Kind::Audio | Kind::Video => return Err("a recording: add it on its own to transcribe it".into()),
        _ => return sniff(&found.path, found.size),
    };
    match body {
        Ok(Body::Lines(text)) => Ok(text),
        Ok(Body::Pages(pages)) => Ok(pages.iter().enumerate().map(|(i, page)| format!("--- page {} ---\n{}", i + 1, page.trim_end())).collect::<Vec<_>>().join("\n")),
        Ok(Body::Cues(cues)) => Ok(cues.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n")),
        Err(Unread::NoWords) => Err("empty".into()),
        Err(Unread::TooBig { size, .. }) => Err(format!("too large to read, {} KB", size >> 10)),
        Err(why) => Err(why.say()),
    }
}

/// Read a folder as one text: what it holds, what was left out and why, then
/// each file under its header. Refused when nothing in it could be read —
/// `InCloud` when that is because its files are all in iCloud.
pub fn read(dir: &Path, scope: &Scope, pdfs: &Pdfs) -> Result<Read, Unread> {
    let meta = std::fs::symlink_metadata(dir).map_err(|e| Unread::Failed(e.to_string()))?;
    if !meta.is_dir() {
        return Err(Unread::Failed("it is not a folder".into()));
    }
    if let Some(why) = scope.refused() {
        return Err(Unread::Failed(format!("it is left out of the knowledge base: {why}")));
    }
    let (found, mut left, stamp) = walk(dir, scope).map_err(Unread::Failed)?;
    let mut read: Vec<(String, String)> = Vec::new();
    let (mut total, mut over, mut in_cloud) = (0, 0, 0);
    for file in &found {
        if file.dataless {
            in_cloud += 1;
            left.push((file.rel.clone(), "in iCloud".into()));
            continue;
        }
        match words(file, pdfs) {
            Ok(text) if read.len() < MOST_FILES && total + text.len() <= MOST_TEXT => {
                total += text.len();
                read.push((file.rel.clone(), text));
            }
            Ok(_) => over += 1,
            Err(why) => left.push((file.rel.clone(), why)),
        }
    }
    if read.is_empty() {
        return Err(match in_cloud > 0 && in_cloud == found.len() {
            true => Unread::InCloud,
            false => Unread::NoWords,
        });
    }
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut text = format!("{name}: a folder of {} file{}, read together.\n\n", read.len(), if read.len() == 1 { "" } else { "s" });
    text.push_str("Its files, in the order they follow:\n");
    for (rel, _) in &read {
        text.push_str(&format!("  {rel}\n"));
    }
    if !left.is_empty() {
        text.push_str("\nLeft out:\n");
        for (rel, why) in left.iter().take(NAMED) {
            text.push_str(&format!("  {rel} — {why}\n"));
        }
        if left.len() > NAMED {
            text.push_str(&format!("  and {} more\n", left.len() - NAMED));
        }
    }
    if over > 0 {
        text.push_str(&format!("\n{over} more file{} not read: one source holds at most {MOST_FILES} files and {} MB of text.\n", if over == 1 { " was" } else { "s were" }, MOST_TEXT >> 20));
    }
    for (rel, words) in &read {
        text.push_str(&format!("\n{}\n{}\n", header(rel), words.trim_end_matches('\n')));
    }
    Ok(Read { text, files: read.len(), stamp })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-folder-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |rel: &str, bytes: &[u8]| {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        };
        write("lab1/main.c", b"#include <stdio.h>\n\nint main(void) {\n    puts(\"hello\");\n}\n");
        write("lab1/Makefile", b"# Build the lab\nmain: main.c\n\tcc -o main main.c\n");
        write("lab1/README.md", b"# Lab 1\n\nWrite a program that says hello.\n");
        write("lab1/main", b"\x7fELF\x02\x01\x01\0\0\0binary");
        write("lab1/diagram.png", b"\x89PNG\r\n");
        write("lab1/build/out.txt", b"generated\n");
        write("lab1/empty.h", b"");
        write("lab1/.vscode/settings.json", b"{}");
        write("README.md", b"# Lab pack\n\nThree labs.\n");
        write("config.json", b"{\"marks\": 10}\n");
        write("lab2/sum.f90", b"program sum\n  print *, 1 + 1\nend program sum\n");
        write("lab2/spec.pdf", &crate::pdf::fixture(&["Lab 2 asks for the sum of two numbers", "Marking scheme for the sum"]));
        dir
    }

    #[test]
    fn a_lab_is_read_as_one_text_its_readme_first_each_file_under_its_name() {
        let dir = lab("read");
        let read = read(&dir, &Scope::anywhere(), &Pdfs::default()).unwrap();
        let headers: Vec<&str> = read.text.lines().filter_map(headed).collect();
        assert_eq!(headers, ["README.md", "config.json", "lab1/README.md", "lab1/main.c", "lab1/Makefile", "lab2/spec.pdf", "lab2/sum.f90"], "{}", read.text);
        assert_eq!(read.files, 7);
        assert!(read.text.starts_with(&format!("{}: a folder of 7 files, read together.", dir.file_name().unwrap().to_string_lossy())), "{}", read.text);
        // Each file's words follow its header, whole.
        assert!(read.text.contains("==> lab1/Makefile <==\n# Build the lab\nmain: main.c\n\tcc -o main main.c\n"));
        assert!(read.text.contains("==> lab2/spec.pdf <==\n--- page 1 ---\n") && read.text.contains("Lab 2 asks for the sum of two numbers\n--- page 2 ---\n"), "{}", read.text);
        // What was left out is said, with why; what is hidden is not.
        for (rel, why) in [("lab1/main", "not text"), ("lab1/diagram.png", "a picture"), ("lab1/build/", "build/"), ("lab1/empty.h", "empty")] {
            assert!(read.text.lines().any(|l| l.starts_with(&format!("  {rel} — ")) && l.contains(why)), "{rel}: {}", read.text);
        }
        assert!(!read.text.contains(".vscode") && !read.text.contains("generated"));
        // The stamp counts what the walk let in, read or not.
        assert_eq!(read.stamp.files, 10);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_change_moves_the_stamp_and_the_rules_of_the_root_apply() {
        let dir = lab("stamp");
        let before = stamp(&dir, &Scope::anywhere()).unwrap();
        std::fs::write(dir.join("lab1/main.c"), b"int main(void) { return 0; }\n").unwrap();
        assert_ne!(stamp(&dir, &Scope::anywhere()).unwrap(), before);

        // Within a root, the person's rules: written from the root, by its label.
        let rel = dir.file_name().unwrap().to_string_lossy().into_owned();
        let rules = Rules::new([format!("Courses/{rel}/lab2/")]);
        let read = read(&dir, &Scope::within(&rules, "Courses", &rel), &Pdfs::default()).unwrap();
        assert!(!read.text.lines().any(|l| headed(l).is_some_and(|h| h.starts_with("lab2/"))), "{}", read.text);
        assert!(read.text.contains("  lab2/ — "), "said to be left out: {}", read.text);
        let refused = Scope::within(&Rules::new([format!("Courses/{rel}/")]), "Courses", &rel);
        assert!(super::read(&dir, &refused, &Pdfs::default()).is_err_and(|e| e.say().contains("left out")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_folder_with_nothing_to_read_or_too_much_is_refused() {
        let dir = std::env::temp_dir().join(format!("proton-folder-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.out"), b"\0\0\0").unwrap();
        assert_eq!(read(&dir, &Scope::anywhere(), &Pdfs::default()), Err(Unread::NoWords));
        assert!(read(&dir.join("a.out"), &Scope::anywhere(), &Pdfs::default()).is_err());
        assert_eq!(headed("==> a b/c.rs <=="), Some("a b/c.rs"));
        assert_eq!(headed("==>  <=="), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
