//! Starting a kernel on macOS without forking.
//!
//! A kernel leads a session of its own (see `kernel::launch`). std can only
//! arrange that by calling `setsid` between a fork and the exec — its own
//! `setsid` is not stable — and on macOS a fork of a process with threads can
//! die before it gets that far. The child runs libSystem's fork handlers, and
//! one that meets a once-only initialisation another thread was halfway
//! through when the process forked aborts: "os_once_t is corrupt", killed,
//! having run nothing of its own. Resolving an address for the first time is
//! one — zmq does that as a kernel's sockets connect — so a kernel started
//! beside another one's connecting could die unheard. One test lost a kernel
//! that way in about a thousand runs.
//!
//! `posix_spawn` runs nothing of this process's in the child, and with
//! `POSIX_SPAWN_SETSID` the kernel leads its session all the same. What std
//! would have done besides is done here too: the program found on the
//! kernel's PATH, no signal blocked and SIGPIPE not ignored, its stdin empty
//! and its stdout and stderr piped.

use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

/// From `<spawn.h>`; the libc crate has it for Linux only.
const POSIX_SPAWN_SETSID: libc::c_int = 0x0400;

extern "C" {
    /// From `<spawn.h>` since macOS 10.15; not in the libc crate for Apple's
    /// systems.
    fn posix_spawn_file_actions_addchdir_np(
        actions: *mut libc::posix_spawn_file_actions_t,
        path: *const libc::c_char,
    ) -> libc::c_int;
}

/// A started kernel's process, as much of a [`std::process::Child`] as a
/// kernel uses, under the same names.
pub(crate) struct Process {
    pid: libc::pid_t,
    /// Known once it has been waited for, after which its pid can be somebody
    /// else's: nothing is sent to it again.
    status: Option<ExitStatus>,
    /// Held as std holds a piped stdout, and as unread: a kernel speaks on its
    /// sockets.
    _stdout: File,
    pub stderr: Option<File>,
}

impl Process {
    pub fn id(&self) -> u32 {
        self.pid as u32
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        let mut raw = 0;
        // SAFETY: waits for this child alone, and does not block.
        match unsafe { libc::waitpid(self.pid, &mut raw, libc::WNOHANG) } {
            0 => Ok(None),
            -1 => Err(io::Error::last_os_error()),
            _ => Ok(Some(*self.status.insert(ExitStatus::from_raw(raw)))),
        }
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        let mut raw = 0;
        // SAFETY: waits for this child alone.
        while unsafe { libc::waitpid(self.pid, &mut raw, 0) } == -1 {
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::Interrupted {
                return Err(err);
            }
        }
        Ok(*self.status.insert(ExitStatus::from_raw(raw)))
    }

    pub fn kill(&mut self) -> io::Result<()> {
        if self.status.is_some() {
            return Ok(());
        }
        // SAFETY: a signal to this child, which has not been waited for, so
        // its pid is still its own.
        match unsafe { libc::kill(self.pid, libc::SIGKILL) } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

/// Start `program` with `args` in `cwd`, with `path` for its PATH when there
/// is one, leading a session of its own.
pub(crate) fn spawn(program: &str, args: &[String], cwd: &Path, path: Option<&OsStr>) -> io::Result<Process> {
    let found = find(Path::new(program), path)?;
    let exe = CString::new(found.into_os_string().into_vec())?;
    let argv = std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(CString::new)
        .collect::<Result<Vec<_>, _>>()?;
    let mut env = Vec::new();
    for (key, value) in std::env::vars_os() {
        if path.is_some() && key == "PATH" {
            continue;
        }
        env.push(entry(&key, &value)?);
    }
    if let Some(path) = path {
        env.push(entry(OsStr::new("PATH"), path)?);
    }
    let dir = CString::new(cwd.as_os_str().as_bytes())?;
    let (stdout, stdout_end) = pipe()?;
    let (stderr, stderr_end) = pipe()?;

    // SAFETY: each of the two is initialised before it is used and destroyed
    // by its guard — on Apple's systems each is a pointer to what its `init`
    // allocated, so it can be moved into one — and every pointer handed over
    // is to a string or list that outlives the call, each list ending in a
    // null.
    unsafe {
        let mut actions = MaybeUninit::uninit();
        check(libc::posix_spawn_file_actions_init(actions.as_mut_ptr()))?;
        let mut actions = Actions(actions.assume_init());
        check(posix_spawn_file_actions_addchdir_np(&mut actions.0, dir.as_ptr()))?;
        check(libc::posix_spawn_file_actions_addopen(
            &mut actions.0,
            0,
            c"/dev/null".as_ptr(),
            libc::O_RDONLY,
            0,
        ))?;
        check(libc::posix_spawn_file_actions_adddup2(&mut actions.0, stdout_end.as_raw_fd(), 1))?;
        check(libc::posix_spawn_file_actions_adddup2(&mut actions.0, stderr_end.as_raw_fd(), 2))?;

        let mut attributes = MaybeUninit::uninit();
        check(libc::posix_spawnattr_init(attributes.as_mut_ptr()))?;
        let mut attributes = Attributes(attributes.assume_init());
        // Nothing blocked, and SIGPIPE back to its default, which this
        // process — like any Rust program — ignores.
        let mut signals = MaybeUninit::uninit();
        libc::sigemptyset(signals.as_mut_ptr());
        check(libc::posix_spawnattr_setsigmask(&mut attributes.0, signals.as_ptr()))?;
        libc::sigaddset(signals.as_mut_ptr(), libc::SIGPIPE);
        check(libc::posix_spawnattr_setsigdefault(&mut attributes.0, signals.as_ptr()))?;
        let flags = POSIX_SPAWN_SETSID | libc::POSIX_SPAWN_SETSIGMASK | libc::POSIX_SPAWN_SETSIGDEF;
        check(libc::posix_spawnattr_setflags(&mut attributes.0, flags as libc::c_short))?;

        let (argv_list, env_list) = (list(&argv), list(&env));
        let mut pid = 0;
        check(libc::posix_spawn(
            &mut pid,
            exe.as_ptr(),
            &actions.0,
            &attributes.0,
            argv_list.as_ptr(),
            env_list.as_ptr(),
        ))?;
        // The pipes' other ends, the child's now, close here as they drop.
        Ok(Process {
            pid,
            status: None,
            _stdout: stdout,
            stderr: Some(stderr),
        })
    }
}

/// Where `program` is: itself when it names a folder, or else the first by
/// that name on `path` — the kernel's PATH, which an exec after a fork would
/// have searched — or on this process's when it has none.
fn find(program: &Path, path: Option<&OsStr>) -> io::Result<PathBuf> {
    if program.as_os_str().as_bytes().contains(&b'/') {
        return Ok(program.to_path_buf());
    }
    let path = path.map(OsStr::to_os_string).or_else(|| std::env::var_os("PATH")).unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(program))
        .find(|candidate| {
            std::fs::metadata(candidate).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
        .ok_or_else(|| io::Error::from_raw_os_error(libc::ENOENT))
}

/// `KEY=value`, as an environment lists it.
fn entry(key: &OsStr, value: &OsStr) -> io::Result<CString> {
    let mut bytes = key.as_bytes().to_vec();
    bytes.push(b'=');
    bytes.extend_from_slice(value.as_bytes());
    Ok(CString::new(bytes)?)
}

/// The pointers to `strings`, ending in a null, as `posix_spawn` takes a list.
fn list(strings: &[CString]) -> Vec<*mut libc::c_char> {
    strings
        .iter()
        .map(|string| string.as_ptr() as *mut libc::c_char)
        .chain(std::iter::once(std::ptr::null_mut()))
        .collect()
}

/// A pipe whose ends are closed in anything this process execs: the end this
/// process reads, and the end the child is given as one of its own.
fn pipe() -> io::Result<(File, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: writes two descriptors into an array of two.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both are new and nobody else's.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for fd in [&read, &write] {
        // SAFETY: sets a flag on a descriptor this function owns.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok((File::from(read), write))
}

/// What a `posix_spawn` function answers: zero, or the error itself.
fn check(code: libc::c_int) -> io::Result<()> {
    match code {
        0 => Ok(()),
        code => Err(io::Error::from_raw_os_error(code)),
    }
}

struct Actions(libc::posix_spawn_file_actions_t);

impl Drop for Actions {
    fn drop(&mut self) {
        // SAFETY: initialised when this was made, and destroyed only here.
        unsafe {
            libc::posix_spawn_file_actions_destroy(&mut self.0);
        }
    }
}

struct Attributes(libc::posix_spawnattr_t);

impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: initialised when this was made, and destroyed only here.
        unsafe {
            libc::posix_spawnattr_destroy(&mut self.0);
        }
    }
}
