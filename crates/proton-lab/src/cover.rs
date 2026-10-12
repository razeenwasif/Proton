//! A book's cover for the PDF mode's library: its first page drawn small by
//! Poppler's `pdftoppm`, as a JPEG, in the sandbox — reading the book and
//! Poppler, writing only a folder of its own, which goes when it is done —
//! and given up on after a while, since a broken PDF can keep a renderer
//! busy for ever. Nothing is fetched.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::sandbox::{self, Kind};

/// How wide a cover is drawn, in pixels: twice what a shelf shows it at.
pub const WIDE: u32 = 300;

/// How long a cover may take.
const LONGEST: Duration = Duration::from_secs(20);

/// Draw `pdf`'s first page, `WIDE` pixels across, as JPEG bytes; working in
/// a folder made under `parent`.
pub fn draw(pdftoppm: &Path, pdf: &Path, parent: &Path, kind: Kind) -> Result<Vec<u8>, String> {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let dir = parent.join(format!("cover-{}-{}", std::process::id(), MADE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let drawn = drawn_in(pdftoppm, pdf, &dir, kind);
    let _ = std::fs::remove_dir_all(&dir);
    drawn
}

fn drawn_in(pdftoppm: &Path, pdf: &Path, dir: &Path, kind: Kind) -> Result<Vec<u8>, String> {
    let args: Vec<String> = ["-f", "1", "-l", "1", "-singlefile", "-scale-to-x", &WIDE.to_string(), "-scale-to-y", "-1", "-jpeg", "-jpegopt", "quality=80"]
        .iter()
        .map(|s| s.to_string())
        .chain([sandbox::real(pdf).display().to_string(), "cover".to_string()])
        .collect();
    // The book's folder, and Poppler's — its libraries beside it.
    let mut reads: Vec<PathBuf> = pdf.parent().map(|p| vec![p.to_path_buf()]).unwrap_or_default();
    if let Some(prefix) = pdftoppm.parent().and_then(Path::parent) {
        reads.push(prefix.to_path_buf());
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let (program, args) = sandbox::wrap_reading(kind, dir, &reads, &home, pdftoppm, &args);
    let mut command = Command::new(&program);
    command
        .args(&args)
        .current_dir(sandbox::real(dir))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", sandbox::real(dir))
        .env("TMPDIR", sandbox::real(dir))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| format!("{}: {e}", program.display()))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > LONGEST => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("drawing its cover took too long".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(e.to_string()),
        }
    };
    if !status.success() {
        return Err(format!("pdftoppm could not draw it ({status})"));
    }
    match std::fs::read(dir.join("cover.jpg")) {
        Ok(bytes) if !bytes.is_empty() => Ok(bytes),
        _ => Err("pdftoppm drew nothing".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-cover-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn stand_in(dir: &Path, body: &str) -> PathBuf {
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let tool = bin.join("pdftoppm");
        std::fs::write(&tool, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        tool
    }

    #[test]
    fn a_cover_is_drawn_into_a_folder_of_its_own_which_goes_when_it_is_done() {
        let dir = scratch("drawn");
        std::fs::write(dir.join("book.pdf"), b"%PDF-1.7").unwrap();
        // The page asked for, the width, and where: printed into the cover.
        let tool = stand_in(&dir, "for last; do :; done; printf 'JPEG %s %s %s' \"$2\" \"$7\" \"$last\" > \"$last.jpg\"");
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let bytes = draw(&tool, &dir.join("book.pdf"), &work, Kind::None).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "JPEG 1 300 cover");
        assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0, "its folder was left behind");
        let failing = stand_in(&dir, "exit 1");
        assert!(draw(&failing, &dir.join("book.pdf"), &work, Kind::None).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[ignore = "runs the machine's pdftoppm"]
    fn a_real_book_has_a_jpeg_cover() {
        let Some(tools) = crate::ocr::OcrTools::find() else { return };
        let dir = scratch("live");
        std::fs::write(dir.join("book.pdf"), include_bytes!("../../../tests/ui/fixtures/three-pages.pdf")).unwrap();
        let tool = tools.pdftoppm;
        let bytes = draw(&tool, &dir.join("book.pdf"), &dir, sandbox::available()).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "a JPEG");
        let _ = std::fs::remove_dir_all(dir);
    }
}
