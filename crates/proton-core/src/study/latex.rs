//! A `.tex` file a tutor wrote, compiled to a PDF by the machine's own
//! pdflatex — when the student asks, with a button, never on its own.
//!
//! It runs in the notebook's `files/`, its working files in `files/.build/`
//! so the folder holds the `.tex` and the PDF and nothing else, with no
//! shell-escape and no input, on a thread, and is stopped if it runs past
//! two minutes. What went wrong is said from the log: its first error, with
//! its line.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The longest a compile may take.
const DEADLINE: Duration = Duration::from_secs(120);

/// A compile under way: its file, and how it ended once it has.
pub struct Compile {
    pub name: String,
    ended: Arc<Mutex<Option<Result<(), String>>>>,
}

impl Compile {
    /// How it ended, once it has: taken, so it is said once.
    pub fn ended(&self) -> Option<Result<(), String>> {
        self.ended.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}

/// The pdflatex to run: on the PATH, or where MacTeX, a TinyTeX of the
/// person's own or Quark's keeps one.
pub fn pdflatex() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "pdflatex.exe" } else { "pdflatex" };
    let mut places: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    places.push(PathBuf::from("/Library/TeX/texbin"));
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for tiny in [home.join("Library/TinyTeX/bin"), home.join(".TinyTeX/bin"), home.join(".quark/tex/TinyTeX/bin")] {
            if let Ok(entries) = std::fs::read_dir(&tiny) {
                places.extend(entries.flatten().map(|e| e.path()));
            }
        }
    }
    if let Ok(years) = std::fs::read_dir("/usr/local/texlive") {
        for year in years.flatten() {
            if let Ok(entries) = std::fs::read_dir(year.path().join("bin")) {
                places.extend(entries.flatten().map(|e| e.path()));
            }
        }
    }
    places.into_iter().map(|dir| dir.join(exe)).find(|p| p.is_file())
}

/// Start compiling `files/<name>`; the PDF lands beside it as `<stem>.pdf`.
pub fn start(files: &Path, name: &str) -> Result<Compile, String> {
    let tex = files.join(name);
    if !tex.is_file() || !name.ends_with(".tex") {
        return Err(format!("{name}: no such .tex file in the notebook's files"));
    }
    let program = pdflatex().ok_or("No pdflatex was found on this Mac: install MacTeX or TinyTeX, or LaTeX in Quark, and try again.")?;
    let build = files.join(".build");
    std::fs::create_dir_all(&build).map_err(|e| format!("{}: {e}", build.display()))?;
    let stem = name.trim_end_matches(".tex").to_string();
    let mut child = Command::new(&program)
        .args(["-interaction=nonstopmode", "-halt-on-error", "-no-shell-escape", "-file-line-error"])
        .arg(format!("-output-directory={}", build.display()))
        .arg(name)
        .current_dir(files)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let ended = Arc::new(Mutex::new(None));
    let done = Arc::clone(&ended);
    let files = files.to_path_buf();
    std::thread::spawn(move || {
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if started.elapsed() > DEADLINE => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => break None,
            }
        };
        let made = build.join(format!("{stem}.pdf"));
        let result = match status {
            None => Err(format!("pdflatex took longer than {} s and was stopped.", DEADLINE.as_secs())),
            Some(status) if status.success() && made.is_file() => {
                std::fs::copy(&made, files.join(format!("{stem}.pdf"))).map(|_| ()).map_err(|e| format!("{stem}.pdf: {e}"))
            }
            Some(_) => Err(first_error(&std::fs::read_to_string(build.join(format!("{stem}.log"))).unwrap_or_default())),
        };
        *done.lock().unwrap_or_else(|p| p.into_inner()) = Some(result);
    });
    Ok(Compile { name: name.to_string(), ended })
}

/// The first error a pdflatex log reports, with its line: `file:12: …` as
/// `-file-line-error` writes it, or a `!` line and the `l.12` after it.
pub fn first_error(log: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if let Some((at, what)) = line.split_once(".tex:").and_then(|(_, rest)| rest.split_once(": ")) {
            if at.chars().all(|c| c.is_ascii_digit()) && !at.is_empty() {
                return format!("line {at}: {}", what.trim());
            }
        }
        if let Some(what) = line.strip_prefix("! ") {
            let at = lines[i..].iter().take(12).find_map(|l| l.strip_prefix("l.").and_then(|r| r.split_whitespace().next()));
            return match at {
                Some(n) => format!("line {n}: {}", what.trim()),
                None => what.trim().to_string(),
            };
        }
    }
    "pdflatex stopped without saying why; its log is in the notebook's files/.build/.".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_error_is_said_with_its_line() {
        let log = "This is pdfTeX\n(./sheet.tex\n./sheet.tex:14: Undefined control sequence.\nl.14 \\foo\n";
        assert_eq!(first_error(log), "line 14: Undefined control sequence.");
        let plain = "! LaTeX Error: File `extarticle.cls' not found.\n\nType X to quit\nl.1 \\documentclass";
        assert_eq!(first_error(plain), "line 1: LaTeX Error: File `extarticle.cls' not found.");
        assert!(first_error("").contains("files/.build"));
    }

    /// With the machine's pdflatex, if it has one: a file compiled to the PDF
    /// beside it, and a broken one said why.
    #[test]
    fn a_tex_file_compiles_to_a_pdf_beside_it_where_there_is_a_pdflatex() {
        if pdflatex().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("proton-latex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sheet.tex"), "\\documentclass{article}\n\\begin{document}\nHello $x^2$.\n\\end{document}\n").unwrap();
        std::fs::write(dir.join("broken.tex"), "\\documentclass{article}\n\\begin{document}\n\\nosuchcommand\n\\end{document}\n").unwrap();
        let wait = |c: Compile| loop {
            if let Some(result) = c.ended() {
                break result;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(wait(start(&dir, "sheet.tex").unwrap()), Ok(()));
        assert!(dir.join("sheet.pdf").is_file() && !dir.join("sheet.aux").exists(), "the PDF beside it, its working files out of the way");
        let why = wait(start(&dir, "broken.tex").unwrap()).unwrap_err();
        assert!(why.starts_with("line 3: Undefined control sequence"), "{why}");
        assert!(start(&dir, "missing.tex").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
