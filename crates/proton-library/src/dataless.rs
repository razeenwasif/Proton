//! Files whose contents are in iCloud rather than on this Mac.
//!
//! ## What a dataless file is
//!
//! With "Optimise Mac Storage" on, iCloud Drive keeps a file's name, size and
//! dates on the disk and its bytes on Apple's servers. The file looks like any
//! other to `stat` — the size it reports is the real one — and the only sign
//! is a flag, `SF_DATALESS`, in `st_flags`. Opening it and reading starts a
//! download, which the reader waits on.
//!
//! An archive of a degree in iCloud is thousands of such files: 8,582 of the
//! 8,583 in the one this was written against, 3.2 GB. A walk that read them
//! would download all of it without anybody having asked.
//!
//! ## Two defences
//!
//! - **Look before reading.** [`is_dataless`] reads the flag from metadata
//!   the walk already has, so an offloaded file is listed — with its size,
//!   under a cloud — and never opened.
//! - **Refuse downloads on the thread anyway.** [`NoDownloads`] sets the
//!   thread's I/O policy so that a read which slips past the first defence
//!   fails rather than fetching: `setiopolicy_np` with
//!   `IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES` set to `OFF`. Its opposite,
//!   [`Downloads`], is for the one place a download is wanted: a file the
//!   person chose to bring down, having been told its size.
//!
//! Both are macOS-only. Elsewhere nothing is dataless and both guards do
//! nothing.

/// `SF_DATALESS`, from `<sys/stat.h>`: the file's contents are not local.
pub const SF_DATALESS: u32 = 0x4000_0000;

/// Whether a file's contents are in iCloud rather than on the disk, from
/// metadata already in hand (`lstat`, never following a link).
pub fn is_dataless(meta: &std::fs::Metadata) -> bool {
    flags(meta) & SF_DATALESS != 0
}

/// A file's BSD flags: `st_flags`. Zero where there is no such thing.
pub fn flags(meta: &std::fs::Metadata) -> u32 {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        meta.st_flags()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = meta;
        0
    }
}

#[cfg(target_os = "macos")]
mod policy {
    use std::ffi::c_int;

    // From `<sys/resource.h>`, checked against the macOS SDK.
    pub const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;
    pub const IOPOL_SCOPE_PROCESS: c_int = 0;
    pub const IOPOL_SCOPE_THREAD: c_int = 1;
    pub const OFF: c_int = 1;
    pub const ON: c_int = 2;

    extern "C" {
        fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
        fn getiopolicy_np(iotype: c_int, scope: c_int) -> c_int;
    }

    pub fn set(scope: c_int, policy: c_int) -> Result<(), String> {
        // SAFETY: plain integers in, an integer out; no memory is shared.
        match unsafe { setiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, scope, policy) } {
            0 => Ok(()),
            _ => Err(format!("setiopolicy_np: {}", std::io::Error::last_os_error())),
        }
    }

    pub fn get(scope: c_int) -> c_int {
        // SAFETY: as above.
        unsafe { getiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, scope) }
    }
}

/// While held, reading an offloaded file on this thread fails instead of
/// downloading it. The thread's earlier policy comes back when it is dropped.
#[must_use = "the policy lasts only as long as the guard"]
pub struct NoDownloads {
    #[cfg(target_os = "macos")]
    was: std::ffi::c_int,
}

impl NoDownloads {
    /// Refuse downloads on this thread. Failing to set the policy is said, not
    /// hidden: the walk that asked still never opens a file the flag marks.
    pub fn on_this_thread() -> Result<NoDownloads, String> {
        #[cfg(target_os = "macos")]
        {
            let was = policy::get(policy::IOPOL_SCOPE_THREAD);
            policy::set(policy::IOPOL_SCOPE_THREAD, policy::OFF)?;
            Ok(NoDownloads { was })
        }
        #[cfg(not(target_os = "macos"))]
        Ok(NoDownloads {})
    }
}

impl Drop for NoDownloads {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.was >= 0 {
            let _ = policy::set(policy::IOPOL_SCOPE_THREAD, self.was);
        }
    }
}

/// While held, reading an offloaded file on this thread downloads it — for a
/// file the person chose to bring down. The earlier policy comes back when it
/// is dropped.
#[must_use = "the policy lasts only as long as the guard"]
pub struct Downloads {
    #[cfg(target_os = "macos")]
    was: std::ffi::c_int,
}

impl Downloads {
    pub fn on_this_thread() -> Result<Downloads, String> {
        #[cfg(target_os = "macos")]
        {
            let was = policy::get(policy::IOPOL_SCOPE_THREAD);
            policy::set(policy::IOPOL_SCOPE_THREAD, policy::ON)?;
            Ok(Downloads { was })
        }
        #[cfg(not(target_os = "macos"))]
        Ok(Downloads {})
    }
}

impl Drop for Downloads {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.was >= 0 {
            let _ = policy::set(policy::IOPOL_SCOPE_THREAD, self.was);
        }
    }
}

/// Refuse downloads for the whole process, for good: for a process whose only
/// work is reading on someone else's behalf, like the MCP server a model
/// searches through. A thread may still take [`Downloads`] for itself.
pub fn no_downloads_in_this_process() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return policy::set(policy::IOPOL_SCOPE_PROCESS, policy::OFF);
    #[cfg(not(target_os = "macos"))]
    Ok(())
}

/// This thread's policy, as `getiopolicy_np` says it: 1 refuses, 2 downloads,
/// 0 is the system's default. `None` where there is no such thing.
pub fn thread_policy() -> Option<i32> {
    #[cfg(target_os = "macos")]
    return Some(policy::get(policy::IOPOL_SCOPE_THREAD));
    #[cfg(not(target_os = "macos"))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_file_is_not_dataless() {
        let path = std::env::temp_dir().join(format!("proton-dataless-{}", std::process::id()));
        std::fs::write(&path, "here, on the disk").unwrap();
        let meta = std::fs::symlink_metadata(&path).unwrap();
        assert!(!is_dataless(&meta));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn the_guards_set_the_threads_policy_and_put_it_back() {
        // On a thread of its own, so nothing else here sees the policy move.
        std::thread::spawn(|| {
            let before = thread_policy().unwrap();
            {
                let _no = NoDownloads::on_this_thread().expect("the policy is settable");
                assert_eq!(thread_policy(), Some(1), "downloads are refused");
                {
                    let _yes = Downloads::on_this_thread().unwrap();
                    assert_eq!(thread_policy(), Some(2), "a chosen download is allowed");
                }
                assert_eq!(thread_policy(), Some(1), "and refused again after");
            }
            assert_eq!(thread_policy(), Some(before), "and as it was once dropped");
        })
        .join()
        .unwrap();
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn elsewhere_the_guards_do_nothing() {
        let _no = NoDownloads::on_this_thread().unwrap();
        let _yes = Downloads::on_this_thread().unwrap();
        assert_eq!(thread_policy(), None);
        assert!(no_downloads_in_this_process().is_ok());
    }
}
