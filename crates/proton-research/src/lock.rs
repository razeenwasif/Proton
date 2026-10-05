//! One Proton at a time drives an investigation.
//!
//! Two windows running rounds over one folder would both number the next
//! hypothesis H008 and write over each other. The one that opens it first
//! holds `.investigation/lock` and keeps its heartbeat fresh; another sees
//! it held and opens the folder to read only. A holder that stopped beating
//! — quit, crashed, the laptop slept — is taken over after [`STALE`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Seconds without a heartbeat after which a lock is anyone's.
pub const STALE: u64 = 120;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Holder {
    pub instance: String,
    pub pid: u32,
    pub host: String,
    /// Seconds since the epoch.
    pub heartbeat: u64,
}

/// The lock, while this instance holds it. Let go of on drop.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    instance: String,
}

fn path(folder: &Path) -> PathBuf {
    folder.join(".investigation").join("lock")
}

fn read(path: &Path) -> Option<Holder> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn host() -> String {
    std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_default()
}

impl Lock {
    /// Take the lock for `instance`, unless another holds it and is alive.
    pub fn acquire(folder: &Path, instance: &str, now: u64) -> Result<Lock, Holder> {
        let path = path(folder);
        if let Some(holder) = read(&path) {
            if holder.instance != instance && now.saturating_sub(holder.heartbeat) < STALE {
                return Err(holder);
            }
        }
        let lock = Lock { path, instance: instance.to_string() };
        lock.beat(now);
        Ok(lock)
    }

    /// Say this instance is still here.
    pub fn beat(&self, now: u64) {
        let holder = Holder { instance: self.instance.clone(), pid: std::process::id(), host: host(), heartbeat: now };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = crate::store::atomic_write(&self.path, serde_json::to_string(&holder).unwrap_or_default().as_bytes());
    }

    /// Whether it is still this instance's: another may have taken it over
    /// while this one was asleep.
    pub fn held(&self) -> bool {
        read(&self.path).is_some_and(|holder| holder.instance == self.instance)
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if self.held() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-lock-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_held_lock_keeps_another_out_until_it_goes_stale() {
        let dir = folder("held");
        let first = Lock::acquire(&dir, "one", 1000).unwrap();
        let refused = Lock::acquire(&dir, "two", 1000 + STALE - 1).unwrap_err();
        assert_eq!(refused.instance, "one");
        let taken = Lock::acquire(&dir, "two", 1000 + STALE).unwrap();
        assert!(taken.held() && !first.held(), "the stale holder lost it");
        drop(first);
        assert!(taken.held(), "letting go of a lock that is not yours leaves it");
        drop(taken);
        assert!(!path(&dir).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_same_instance_takes_its_own_lock_again() {
        let dir = folder("again");
        let first = Lock::acquire(&dir, "one", 10).unwrap();
        let again = Lock::acquire(&dir, "one", 11).unwrap();
        std::mem::forget(first);
        assert!(again.held());
        let _ = std::fs::remove_dir_all(dir);
    }
}
