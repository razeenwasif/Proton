//! One Proton at a time writes a notebook.
//!
//! Two windows adding sources to one notebook would both number the next
//! one `S08`. The one that opens it first holds `.notebook/lock` and keeps
//! its heartbeat fresh, exactly as an investigation's lock is kept; another
//! opens the notebook to read only.

use std::path::{Path, PathBuf};

pub use proton_research::lock::{Holder, Lock, STALE};

/// Where a notebook's lock is kept.
pub fn file(dir: &Path) -> PathBuf {
    dir.join(".notebook").join("lock")
}

/// Take a notebook's lock for `instance`, unless another holds it and is alive.
pub fn acquire(dir: &Path, instance: &str, now: u64) -> Result<Lock, Holder> {
    Lock::acquire_file(file(dir), instance, now)
}

/// Who holds a notebook's lock, if anyone alive does.
pub fn holder(dir: &Path, now: u64) -> Option<Holder> {
    proton_research::lock::holder_of(&file(dir), now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_proton_reads_only_until_the_first_goes_quiet() {
        let dir = std::env::temp_dir().join(format!("proton-study-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = acquire(&dir, "one", 1000).unwrap();
        assert!(file(&dir).is_file());
        assert_eq!(acquire(&dir, "two", 1000 + STALE - 1).unwrap_err().instance, "one");
        assert_eq!(holder(&dir, 1001).map(|h| h.instance), Some("one".into()));
        drop(first);
        assert!(holder(&dir, 1002).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
