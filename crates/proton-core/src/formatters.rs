//! Formatters run as programs, for a file no language server will format:
//! `clang-format` for C and C++ when clangd is not here, or not answering.
//!
//! It runs off the core's thread — reading the buffer as it is, unsaved
//! edits and all, on stdin — and what it prints is turned into the smallest
//! edit that makes the buffer match: the lines it changed, not the whole
//! file, so the cursor stays where it was and one `u` takes it all back.
//! The answer goes through the same door a server's formatting does, so a
//! buffer edited while it was formatting is left alone.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::OnceLock;

/// The files clang-format is asked to format.
const C_FAMILY: &[&str] = &["c", "h", "cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx", "h++", "ipp", "tpp", "m", "mm"];

/// Whether this file is one clang-format formats.
pub fn is_c_family(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| C_FAMILY.contains(&e.to_ascii_lowercase().as_str()))
}

/// Where clang-format is, looked for once: on the PATH; then, on a Mac,
/// inside Xcode, which has it but does not put it on the PATH; then where
/// LLVM is usually installed.
pub fn clang_format() -> Option<PathBuf> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let exe = if cfg!(windows) { "clang-format.exe" } else { "clang-format" };
            if let Some(path) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(exe)).find(|p| p.is_file())) {
                return Some(path);
            }
            // xcrun offers to install the developer tools when they are
            // missing, so it is asked only when xcode-select says they are there.
            if cfg!(target_os = "macos") && Command::new("/usr/bin/xcode-select").arg("-p").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) {
                if let Ok(out) = Command::new("/usr/bin/xcrun").args(["--find", "clang-format"]).stderr(Stdio::null()).output() {
                    let found = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
                    if out.status.success() && found.is_file() {
                        return Some(found);
                    }
                }
            }
            let mut known = vec![
                PathBuf::from("/opt/homebrew/opt/llvm/bin/clang-format"),
                PathBuf::from("/usr/local/opt/llvm/bin/clang-format"),
                PathBuf::from(r"C:\Program Files\LLVM\bin\clang-format.exe"),
            ];
            if let Ok(entries) = std::fs::read_dir("/usr/lib") {
                known.extend(entries.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("llvm-")).map(|e| e.path().join("bin/clang-format")));
            }
            known.into_iter().find(|p| p.is_file())
        })
        .clone()
}

/// Format `text` as `path` with `program`, on a thread; the answer — the
/// formatted text, or why not — arrives on the receiver. clang-format reads
/// the project's `.clang-format` from beside the file, and falls back to
/// LLVM's style without one.
pub fn start(program: &Path, path: &Path, text: String) -> Receiver<Result<String, String>> {
    let args = vec![
        format!("--assume-filename={}", path.display()),
        "--style=file".to_string(),
        "--fallback-style=LLVM".to_string(),
    ];
    let cwd = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    run_clang_format(program.to_path_buf(), args, cwd, text)
}

/// Format `text` as `path` inside an attached dev container using `clang-format`.
pub fn start_in_container(
    target: &proton_container::ExecTarget,
    path: &Path,
    text: String,
) -> Receiver<Result<String, String>> {
    let container_path = target
        .path_map
        .host_to_container(path)
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let cf_args = vec![
        format!("--assume-filename={container_path}"),
        "--style=file".to_string(),
        "--fallback-style=LLVM".to_string(),
    ];
    let args = proton_container::runtime::exec_args(target, false, "clang-format", &cf_args);
    run_clang_format(
        target.runtime.clone(),
        args,
        target.path_map.host_root.clone(),
        text,
    )
}

fn run_clang_format(
    program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    text: String,
) -> Receiver<Result<String, String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let answer = (|| {
            let mut child = Command::new(&program)
                .args(&args)
                .current_dir(&cwd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("{}: {e}", program.display()))?;
            // Written on its own thread: a big file fills the pipe before
            // clang-format starts answering, and both would wait.
            let mut stdin = child.stdin.take().ok_or("clang-format has no stdin")?;
            let writer = std::thread::spawn(move || {
                let _ = stdin.write_all(text.as_bytes());
            });
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            let _ = writer.join();
            if !out.status.success() {
                let why = String::from_utf8_lossy(&out.stderr);
                return Err(format!("clang-format: {}", why.lines().next().unwrap_or("it failed").trim()));
            }
            String::from_utf8(out.stdout).map_err(|_| "clang-format answered with something that is not text".to_string())
        })();
        let _ = tx.send(answer);
    });
    rx
}

/// Where the end of `text` is, as LSP counts: its line, and UTF-16 units into it.
fn end_of(text: &str) -> proton_lsp::Position {
    let line = text.matches('\n').count() as u32;
    let last = text.rsplit('\n').next().unwrap_or("");
    proton_lsp::Position { line, character: last.encode_utf16().count() as u32 }
}

/// The smallest edit by whole lines that turns `old` into `new`: the lines
/// between what they share at the start and what they share at the end.
pub fn edits(old: &str, new: &str) -> Vec<proton_lsp::TextEdit> {
    if old == new {
        return Vec::new();
    }
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    let start = if prefix < a.len() { proton_lsp::Position { line: prefix as u32, character: 0 } } else { end_of(old) };
    let end = if suffix > 0 { proton_lsp::Position { line: (a.len() - suffix) as u32, character: 0 } } else { end_of(old) };
    vec![proton_lsp::TextEdit { range: proton_lsp::Range { start, end }, new_text: b[prefix..b.len() - suffix].concat() }]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `old` with `edits` applied, as a buffer would.
    fn apply(old: &str, edits: &[proton_lsp::TextEdit]) -> String {
        let offset = |p: proton_lsp::Position| {
            let mut at = 0;
            for (i, line) in old.split_inclusive('\n').enumerate() {
                if i as u32 == p.line {
                    return at + line.chars().take(p.character as usize).map(char::len_utf8).sum::<usize>();
                }
                at += line.len();
            }
            at
        };
        let mut out = old.to_string();
        for e in edits.iter().rev() {
            let (s, t) = (offset(e.range.start), offset(e.range.end));
            out.replace_range(s..t, &e.new_text);
        }
        out
    }

    #[test]
    fn only_the_lines_that_changed_are_replaced() {
        let old = "int main(){\nreturn 0;\n}\n";
        let new = "int main() {\n  return 0;\n}\n";
        let e = edits(old, new);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].range.start.line, e[0].range.end.line), (0, 2), "the closing brace is kept, not rewritten");
        assert_eq!(apply(old, &e), new);
        for (old, new) in [
            ("a\nb\n", "a\nb\nc\n"),
            ("a\nb", "a\nb\n"),
            ("x\na\n", "a\n"),
            ("a\nb\nc\n", "a\nB\nc\n"),
            ("", "int x;\n"),
            ("é = 1;\n", "é = 2;\n"),
            ("a\r\nb\r\n", "a\nb\n"),
        ] {
            assert_eq!(apply(old, &edits(old, new)), new, "{old:?} → {new:?}");
        }
        assert!(edits("same\n", "same\n").is_empty(), "a formatted file is not touched");
    }

    #[test]
    fn the_c_family_is_known_by_its_extensions() {
        for path in ["a.c", "a.h", "a.cpp", "a.HPP", "a.cc", "a.mm"] {
            assert!(is_c_family(Path::new(path)), "{path}");
        }
        for path in ["a.rs", "Makefile", "a.py"] {
            assert!(!is_c_family(Path::new(path)), "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn clang_format_is_handed_the_buffer_and_answers_with_it_formatted() {
        use std::os::unix::fs::PermissionsExt;
        // A stand-in that indents what it is given, so the test needs no LLVM.
        let dir = std::env::temp_dir().join(format!("proton-fmt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("clang-format");
        std::fs::write(&fake, "#!/bin/sh\ncase \"$1\" in --assume-filename=*.c) sed 's/^return/  return/' ;; *) echo 'wrong file' >&2; exit 2 ;; esac\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let rx = start(&fake, &dir.join("main.c"), "int main() {\nreturn 0;\n}\n".into());
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap().unwrap(), "int main() {\n  return 0;\n}\n");
        let rx = start(&fake, &dir.join("main.rs"), "x".into());
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap().unwrap_err(), "clang-format: wrong file");
        // The real one, where there is one.
        if let Some(real) = clang_format() {
            let rx = start(&real, &dir.join("main.c"), "int main(){int a=1;return a;}\n".into());
            let formatted = rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap().unwrap();
            assert_eq!(formatted, "int main() {\n  int a = 1;\n  return a;\n}\n", "LLVM's style, without a .clang-format");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
