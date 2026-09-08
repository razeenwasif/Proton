//! What extracting the real PDFs actually costs, and how often it fails.
//!
//! Ignored by default and pointed at real directories, because the only
//! useful answer here comes from documents nobody wrote for a test:
//!
//! ```bash
//! PROTON_TEST_VAULT="C:/Users/Razeen/OnyxVault,C:/Users/Razeen/Courses" \
//! cargo test -p proton-library --test pdfs -- --include-ignored --nocapture
//! ```
//!
//! It prints counts and timings and never prints a line of anybody's
//! documents. What comes out of a PDF here is somebody's coursework and
//! medical paperwork; the measurement is how much text and how long, not what
//! it said.

use std::path::PathBuf;
use std::time::Instant;

fn roots() -> Vec<PathBuf> {
    std::env::var("PROTON_TEST_VAULT")
        .unwrap_or_default()
        .split(',')
        .map(|part| PathBuf::from(part.trim()))
        .filter(|path| path.is_dir())
        .collect()
}

fn pdfs(root: &std::path::Path) -> Vec<PathBuf> {
    ignore::WalkBuilder::new(root)
        .hidden(true)
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|k| k.is_file()))
        .map(ignore::DirEntry::into_path)
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        })
        .collect()
}

#[test]
#[ignore = "needs real documents"]
fn what_the_real_pdfs_cost() {
    let roots = roots();
    if roots.is_empty() {
        eprintln!("PROTON_TEST_VAULT names no readable directory — skipping");
        return;
    }

    let mut ok = 0usize;
    let mut failed = Vec::new();
    let mut panicked = Vec::new();
    let mut empty = Vec::new();
    let mut pages = 0usize;
    let mut chars = 0usize;
    let mut bytes = 0u64;
    let mut slowest: Vec<(u128, String)> = Vec::new();
    let mut thinnest: Vec<(usize, String)> = Vec::new();

    let started = Instant::now();
    for root in &roots {
        for path in pdfs(root) {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let one = Instant::now();

            // `pdf-extract` panics on some real files — there is a fork on
            // crates.io whose whole reason for existing is one of them — and a
            // panic on a worker thread would lose the entire walk.
            let out = std::panic::catch_unwind(|| pdf_extract::extract_text_by_pages(&path));
            let took = one.elapsed().as_millis();
            slowest.push((took, format!("{name} ({} KB)", size / 1024)));
            bytes += size;

            match out {
                Ok(Ok(text)) => {
                    let count: usize = text
                        .iter()
                        .map(|p| p.chars().filter(|c| c.is_alphanumeric()).count())
                        .sum();
                    // Everything is kept, and the *distribution* is what says
                    // where the line between a scan and a real document is. An
                    // earlier draft picked 200 characters out of the air, and
                    // it would have thrown away a one-page handout.
                    ok += 1;
                    pages += text.len();
                    chars += count;
                    thinnest.push((count, name.clone()));
                    if count == 0 {
                        empty.push(name);
                    }
                }
                Ok(Err(why)) => failed.push(format!("{name}: {why}")),
                Err(_) => panicked.push(name),
            }
        }
    }
    let elapsed = started.elapsed();

    let total = ok + failed.len() + panicked.len();
    println!("\n{total} PDFs, {} MB, in {elapsed:?}", bytes / 1_048_576);
    println!("  {ok} extracted: {pages} pages, {chars} characters");
    println!("  {} with no text layer (scans)", empty.len());
    println!("  {} refused, {} panicked", failed.len(), panicked.len());
    for why in failed.iter().take(10) {
        println!("    refused: {why}");
    }
    for name in panicked.iter().take(10) {
        println!("    panicked: {name}");
    }

    thinnest.sort();
    println!("  thinnest documents, by letters extracted:");
    for (count, name) in thinnest.iter().take(8) {
        println!("    {count:>7}  {name}");
    }

    slowest.sort_by_key(|(ms, _)| std::cmp::Reverse(*ms));
    println!("  slowest:");
    for (ms, name) in slowest.iter().take(6) {
        println!("    {ms:>7} ms  {name}");
    }
    if total > 0 {
        println!(
            "  {} ms per document on average",
            elapsed.as_millis() / total as u128
        );
    }
}

#[test]
#[ignore = "needs real documents"]
fn how_long_does_a_stat_only_scan_take() {
    // Deciding between a filesystem watcher and a periodic check: if simply
    // asking the disk "has anything changed" is cheap, the periodic check
    // wins on simplicity and adds no dependency.
    let roots = roots();
    if roots.is_empty() {
        eprintln!("PROTON_TEST_VAULT names no readable directory — skipping");
        return;
    }
    for _ in 0..2 {
        let started = Instant::now();
        let mut files = 0usize;
        let mut newest = std::time::UNIX_EPOCH;
        for root in &roots {
            for entry in ignore::WalkBuilder::new(root)
                .hidden(true)
                .build()
                .flatten()
            {
                let Ok(meta) = entry.metadata() else { continue };
                if !meta.is_file() {
                    continue;
                }
                files += 1;
                if let Ok(at) = meta.modified() {
                    newest = newest.max(at);
                }
            }
        }
        println!("  stat-only scan: {files} files in {:?}", started.elapsed());
        let _ = newest;
    }
}
