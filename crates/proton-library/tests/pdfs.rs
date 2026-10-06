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
        // A file in iCloud would be downloaded by reading it.
        .filter(|e| e.metadata().is_ok_and(|m| !proton_library::dataless::is_dataless(&m)))
        .map(ignore::DirEntry::into_path)
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        })
        .collect()
}

/// The words of a text, for comparing two readings of it: lowercase, three
/// letters or more.
fn words(pages: &[String]) -> std::collections::HashSet<String> {
    pages
        .iter()
        .flat_map(|page| page.split(|c: char| !c.is_alphabetic()))
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect()
}

/// Whether `pdftotext` reads the real PDFs as well as `pdf-extract` does, and
/// how much faster. Prints counts, times and file names, never their words.
///
/// ```bash
/// PROTON_TEST_VAULT="$HOME/OnyxVault,$HOME/Courses" \
/// cargo test -p proton-library --test pdfs pdftotext -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs real documents, and pdftotext"]
fn pdftotext_against_pdf_extract() {
    let roots = roots();
    let Some(exe) = proton_library::pdf::find_pdftotext() else {
        eprintln!("no pdftotext — skipping");
        return;
    };
    if roots.is_empty() {
        eprintln!("PROTON_TEST_VAULT names no readable directory — skipping");
        return;
    }
    let (mut ours, mut theirs) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    let (mut both, mut only_pdftotext, mut only_extract, mut neither) = (0, 0, 0, 0);
    let (mut fewer, mut pages_differ) = (Vec::new(), 0);
    let (mut letters_ours, mut letters_theirs) = (0usize, 0usize);
    let mut overlap = Vec::new();
    let mut slow: Vec<(u128, u128, String)> = Vec::new();
    let count = |pages: &[String]| pages.iter().map(|p| p.chars().filter(|c| c.is_alphanumeric()).count()).sum::<usize>();
    for root in &roots {
        for path in pdfs(root) {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let one = Instant::now();
            let a = std::panic::catch_unwind(|| pdf_extract::extract_text_by_pages(&path)).ok().and_then(Result::ok);
            let took_a = one.elapsed();
            let one = Instant::now();
            let b = proton_library::pdf::pdftotext(&exe, &path, proton_library::pdf::PDFTOTEXT_LONGEST);
            let took_b = one.elapsed();
            ours += took_a;
            theirs += took_b;
            slow.push((took_a.as_millis(), took_b.as_millis(), name.clone()));
            match (a, b) {
                (Some(a), Some(b)) => {
                    both += 1;
                    let (la, lb) = (count(&a), count(&b));
                    letters_ours += la;
                    letters_theirs += lb;
                    if (lb as f64) < 0.9 * la as f64 {
                        fewer.push(format!("{name}: {lb} letters against {la}"));
                    }
                    if a.len() != b.len() {
                        pages_differ += 1;
                    }
                    let (wa, wb) = (words(&a), words(&b));
                    let union = wa.union(&wb).count().max(1);
                    overlap.push(wa.intersection(&wb).count() as f64 / union as f64);
                }
                (None, Some(_)) => only_pdftotext += 1,
                (Some(_), None) => only_extract += 1,
                (None, None) => neither += 1,
            }
        }
    }
    let total = both + only_pdftotext + only_extract + neither;
    println!("\n{total} PDFs");
    println!("  pdf-extract: {ours:.1?}; pdftotext: {theirs:.1?} ({:.1}x faster)", ours.as_secs_f64() / theirs.as_secs_f64().max(1e-9));
    println!("  read by both: {both}; only pdftotext: {only_pdftotext}; only pdf-extract: {only_extract}; neither: {neither}");
    println!("  letters: pdf-extract {letters_ours}, pdftotext {letters_theirs}");
    println!("  page counts differ on {pages_differ}");
    overlap.sort_by(f64::total_cmp);
    if !overlap.is_empty() {
        println!(
            "  shared words (Jaccard): median {:.3}, lowest tenth {:.3}, lowest {:.3}",
            overlap[overlap.len() / 2],
            overlap[overlap.len() / 10],
            overlap[0]
        );
    }
    println!("  pdftotext read under 90% of pdf-extract's letters on {}:", fewer.len());
    for line in fewer.iter().take(10) {
        println!("    {line}");
    }
    slow.sort_by_key(|(a, _, _)| std::cmp::Reverse(*a));
    println!("  slowest for pdf-extract (ms, pdf-extract / pdftotext):");
    for (a, b, name) in slow.iter().take(6) {
        println!("    {a:>7} / {b:>6}  {name}");
    }
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
