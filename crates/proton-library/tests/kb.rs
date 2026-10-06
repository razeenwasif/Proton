//! A knowledge base, for real: the roots a person studies from, catalogued
//! and read as a notebook's picker and search read them.
//!
//! Ignored by default and pointed at real folders. Roots are separated by
//! commas; rules, which may hold commas, by semicolons:
//!
//! ```bash
//! PROTON_KB_ROOTS="$HOME/OnyxVault,$HOME/Courses,$HOME/Library/Mobile Documents/com~apple~CloudDocs/ANU" \
//! PROTON_KB_EXCLUDE="OnyxVault/Private/;OnyxVault/Finance/" \
//! cargo test -p proton-library --test kb -- --ignored --nocapture --test-threads 1
//! ```
//!
//! It prints counts and times, never a document's words and never a folder's
//! name. It asserts what matters most: the catalogue takes seconds, nothing a
//! rule leaves out is listed, and every file that was in iCloud before is in
//! iCloud after — a walk that downloaded a degree's archive to list it would
//! be the worst thing this crate could do.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_library::{dataless, pdf, Catalog, Kind, Root, Rules, Shelf, Why};

fn roots() -> Vec<Root> {
    let paths: Vec<PathBuf> = std::env::var("PROTON_KB_ROOTS")
        .unwrap_or_default()
        .split(',')
        .map(|part| PathBuf::from(part.trim()))
        .filter(|path| path.is_dir())
        .collect();
    Root::labelled(paths)
}

fn rules() -> Rules {
    Rules::new(std::env::var("PROTON_KB_EXCLUDE").unwrap_or_default().split(';').map(str::trim).filter(|r| !r.is_empty()))
}

fn what(why: &Why) -> &'static str {
    match why {
        Why::Default(_) => "by default",
        Why::Yours(_) => "by your rules",
        Why::Dataset { .. } => "datasets",
        Why::InCloud => "folders in iCloud",
        Why::Unlistable(_) => "unlistable",
    }
}

/// Every file the catalogue says is in iCloud, checked again now.
fn still_in_icloud(catalog: &Catalog) -> (usize, Vec<String>) {
    let mut downloaded = Vec::new();
    let mut count = 0;
    for entry in catalog.entries().iter().filter(|e| e.dataless) {
        count += 1;
        let meta = std::fs::symlink_metadata(catalog.path(entry)).expect("still there");
        if !dataless::is_dataless(&meta) {
            downloaded.push(catalog.labelled(entry));
        }
    }
    (count, downloaded)
}

#[test]
#[ignore = "needs real folders"]
fn the_knowledge_base_is_catalogued_in_seconds() {
    let roots = roots();
    if roots.is_empty() {
        eprintln!("PROTON_KB_ROOTS names no readable folder — skipping");
        return;
    }
    let rules = rules();
    // Twice: the first walk may find the disk's cache cold.
    let mut catalog = Catalog::walk(&roots, &rules);
    let cold = catalog.elapsed();
    catalog = Catalog::walk(&roots, &rules);
    println!("\ncatalogued in {cold:.2?} cold, {:.2?} warm", catalog.elapsed());

    for (index, root) in catalog.roots().iter().enumerate() {
        let entries: Vec<_> = catalog.entries().iter().filter(|e| e.root == index).collect();
        let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for entry in &entries {
            *kinds.entry(entry.kind.name()).or_default() += 1;
        }
        let in_cloud: Vec<_> = entries.iter().filter(|e| e.dataless).collect();
        let bytes: u64 = in_cloud.iter().map(|e| e.size).sum();
        let mut left: BTreeMap<&str, usize> = BTreeMap::new();
        let mut left_files = 0;
        for skipped in catalog.skipped().iter().filter(|s| s.root == index) {
            *left.entry(what(&skipped.why)).or_default() += 1;
            if let Why::Dataset { files } = skipped.why {
                left_files += files;
            }
        }
        println!(
            "  {}: {} files ({} readable), {} in iCloud ({:.2} GB); left out: {:?} ({} files in datasets)",
            root.label,
            entries.len(),
            entries.iter().filter(|e| e.kind.indexed()).count(),
            in_cloud.len(),
            bytes as f64 / 1e9,
            left,
            left_files
        );
        println!("    kinds: {kinds:?}");
    }

    // Nothing a rule leaves out is listed.
    for entry in catalog.entries() {
        let label = &catalog.roots()[entry.root].label;
        assert!(
            rules.under(label).why_anywhere(&entry.rel, false).is_none(),
            "a file a rule leaves out was listed (in {label})"
        );
    }
    let (count, downloaded) = still_in_icloud(&catalog);
    assert!(downloaded.is_empty(), "{} of {count} files in iCloud were downloaded by listing them", downloaded.len());
    assert!(catalog.elapsed() < Duration::from_secs(10), "a catalogue should take seconds, not {:?}", catalog.elapsed());
}

#[test]
#[ignore = "needs real folders, and minutes the first time"]
fn reading_the_knowledge_base_downloads_nothing() {
    let roots = roots();
    if roots.is_empty() {
        eprintln!("PROTON_KB_ROOTS names no readable folder — skipping");
        return;
    }
    let before = Catalog::walk(&roots, &rules());
    let extractor = match (std::env::var("PROTON_KB_EXTRACTOR").as_deref(), pdf::find_pdftotext()) {
        (Ok("pdf-extract"), _) | (_, None) => pdf::Extractor::PdfExtract,
        (_, Some(exe)) => pdf::Extractor::Pdftotext(exe),
    };
    println!("\nreading PDFs with {extractor:?}");

    let mut shelf = Shelf::new();
    shelf.cache_in(pdf::cache_dir());
    shelf.set_pdfs(true);
    shelf.extract_with(extractor);
    shelf.read_catalog(Some(rules()));
    shelf.set_roots(roots.iter().map(|r| r.path.clone()).collect());

    let started = Instant::now();
    let mut text_at = None;
    let mut said = Instant::now();
    loop {
        shelf.poll();
        if text_at.is_none() && shelf.is_built() {
            text_at = Some(started.elapsed());
            println!("  the text half in {:.1?}: {} documents, {} passages", started.elapsed(), shelf.documents(), shelf.len());
        }
        if shelf.is_built() && !shelf.is_building() {
            break;
        }
        if said.elapsed() > Duration::from_secs(15) {
            let (done, total) = shelf.progress();
            println!("  {done} of {total} read, {:.0?}", started.elapsed());
            said = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let catalog = shelf.catalog().expect("read through a catalogue");
    let pdfs = catalog.readable().filter(|e| e.kind == Kind::Pdf).count();
    println!(
        "  everything in {:.1?}: {} documents ({pdfs} PDFs readable), {} passages, {} notes with tags or links",
        started.elapsed(),
        shelf.documents(),
        shelf.len(),
        shelf.notes().len()
    );

    let (count, downloaded) = still_in_icloud(&before);
    println!("  {count} files were in iCloud before; {} are not now", downloaded.len());
    assert!(downloaded.is_empty(), "reading downloaded {} files from iCloud", downloaded.len());
    // And a search over it answers.
    assert!(!shelf.search("the", 1).is_empty() || shelf.is_empty());
}
