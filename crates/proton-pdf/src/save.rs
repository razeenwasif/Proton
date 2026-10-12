//! A PDF written: as an update appended to the file it was — exactly the
//! objects that differ from the file — or whole; and on disk atomically, so
//! a save cut short leaves the file as it was.

use std::io::Write;
use std::path::Path;

use lopdf::{Document, IncrementalDocument};

/// `current` as an update appended to `disk`, the bytes `on_disk` was read
/// from: every object that is new or differs, the trailer pointing back.
pub fn incremental(disk: &[u8], on_disk: &Document, current: &Document) -> Result<Vec<u8>, String> {
    let mut update = IncrementalDocument::create_from(disk.to_vec(), on_disk.clone());
    for (id, object) in &current.objects {
        if on_disk.objects.get(id) != Some(object) {
            update.new_document.objects.insert(*id, object.clone());
        }
    }
    update.new_document.max_id = current.max_id.max(on_disk.max_id);
    // What the document is now: its catalog and its information.
    for key in [b"Root".as_slice(), b"Info"] {
        match current.trailer.get(key) {
            Ok(value) => update.new_document.trailer.set(key, value.clone()),
            Err(_) => {
                update.new_document.trailer.remove(key);
            }
        }
    }
    let mut out = Vec::with_capacity(disk.len() + 4096);
    update.save_to(&mut out).map_err(|e| format!("the update could not be written: {e}"))?;
    Ok(out)
}

/// `current` written whole.
pub fn whole(current: &Document) -> Result<Vec<u8>, String> {
    let mut doc = current.clone();
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| format!("the PDF could not be written: {e}"))?;
    Ok(out)
}

/// `bytes` written to `path` atomically: to a file beside it, made durable,
/// then put in its place — what was there is left as it was if any step
/// fails.
pub fn atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "document.pdf".into());
    let temporary = dir.join(format!(".{name}.proton-{}", std::process::id()));
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        // The file's own permissions, when it has some.
        if let Ok(meta) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&temporary, meta.permissions());
        }
        std::fs::rename(&temporary, path)?;
        if let Ok(folder) = std::fs::File::open(dir) {
            let _ = folder.sync_all();
        }
        Ok::<(), std::io::Error>(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}
