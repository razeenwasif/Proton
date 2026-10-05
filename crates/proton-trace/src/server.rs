//! Starting the tracer, and finding the Python it needs.
//!
//! Upstream ships a plain `http.server` that wants `python3`, `gcc`, `g++` and
//! a Python-enabled `gdb`. Proton spawns it as a child on loopback and owns
//! its lifetime — the sidecar arrangement, which is worth doing before any UI
//! work because it is where the process supervision, the port handshake and
//! the toolchain detection actually get proven.
//!
//! Two things about that handshake are not obvious and both were found by
//! doing it:
//!
//! - **Python buffers stdout when it is not a terminal.** The server prints
//!   `Code Visualizer -> http://…` when it is ready, and without `-u` that
//!   line sits in a buffer and the handshake waits forever.
//! - **On Windows the toolchain is not on `PATH`.** The `python` there is the
//!   Microsoft Store stub, and the real one is in MSYS2 — which is also where
//!   the `gcc` and Python-enabled `gdb` the backends need live. So the child
//!   is given a `PATH` with MSYS2 in front rather than being asked to find it.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Where MSYS2 puts the toolchain on Windows. Checked before `PATH` because
/// the `python` on `PATH` is usually the Store stub, which is not a Python.
const MSYS2: &str = r"C:\msys64\ucrt64\bin";

/// A running tracer.
pub struct Server {
    child: Child,
    base: String,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").field("base", &self.base).finish()
    }
}

impl Server {
    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// Spawn the tracer from a checkout and wait for it to say it is ready.
    pub fn start(root: &Path) -> Result<Server, String> {
        if !root.join("server").join("app.py").is_file() {
            return Err(format!(
                "no Code Visualizer at {} — expected server/app.py",
                root.display()
            ));
        }
        let python = python().ok_or_else(|| {
            "no python3: install it, or MSYS2 with the ucrt64 toolchain".to_string()
        })?;
        let port = free_port()?;

        let mut command = Command::new(&python);
        command
            .arg("-u") // unbuffered, or the readiness line never arrives
            .arg("-m")
            .arg("server.app")
            .current_dir(root)
            .env("CVIZ_HOST", "127.0.0.1")
            .env("CVIZ_PORT", port.to_string())
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // The backends shell out to gcc and gdb, so they need the same
        // toolchain directory this Python came from.
        //
        // Built with `join_paths` rather than a hand-joined string: `;` only
        // separates a `PATH` on Windows, and a literal `;` in front of a real
        // Unix `PATH` makes the whole variable one unresolvable entry —
        // `gcc`, `gdb` and even `python` itself would stop being found.
        if let Some(dir) = python.parent() {
            let existing = std::env::var_os("PATH").unwrap_or_default();
            let dirs = std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&existing));
            if let Ok(joined) = std::env::join_paths(dirs) {
                command.env("PATH", joined);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command
            .spawn()
            .map_err(|err| format!("could not start the tracer: {err}"))?;

        let base = format!("http://127.0.0.1:{port}");
        match ready(&mut child, Duration::from_secs(20)) {
            Ok(()) => Ok(Server { child, base }),
            Err(why) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(why)
            }
        }
    }

    /// Whether the child is still alive.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // The tracer compiles and runs code. An orphaned one is worse than a
        // leaked port.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Wait for the line that says the port is bound.
///
/// Reading the child's own announcement rather than polling the port: a
/// connection that succeeds tells you something is listening, not that it is
/// the thing you started.
fn ready(child: &mut Child, limit: Duration) -> Result<(), String> {
    let Some(stdout) = child.stdout.take() else {
        return Err("the tracer produced no output".into());
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
    });

    // Its stderr is where a missing gdb or a bad toolchain says so, and that
    // is the message worth repeating rather than "it did not start".
    let complaint = child.stderr.take().map(|stderr| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut said = String::new();
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                said.push_str(&line);
                said.push('\n');
            }
            let _ = tx.send(said);
        });
        rx
    });

    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(250)) {
            if line.contains("Code Visualizer") && line.contains("http://") {
                return Ok(());
            }
            continue;
        }
        if let Ok(Some(status)) = child.try_wait() {
            let said = complaint
                .and_then(|rx| rx.recv_timeout(Duration::from_millis(200)).ok())
                .unwrap_or_default();
            return Err(format!(
                "the tracer exited ({status}){}",
                if said.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", said.trim())
                }
            ));
        }
    }
    Err("the tracer did not become ready".into())
}

/// A loopback port nothing is using.
fn free_port() -> Result<u16, String> {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").map_err(|err| format!("no free port: {err}"))?;
    let port = listener
        .local_addr()
        .map_err(|err| format!("no free port: {err}"))?
        .port();
    drop(listener);
    Ok(port)
}

/// Find a Python that is actually a Python.
///
/// MSYS2 first on Windows: `python` on `PATH` there is a Store stub that
/// prints an advert and exits, and the backends need MSYS2's `gcc` and
/// Python-enabled `gdb` anyway.
pub fn python() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if cfg!(windows) {
        vec![
            Path::new(MSYS2).join("python3.exe"),
            Path::new(MSYS2).join("python.exe"),
        ]
    } else {
        Vec::new()
    };

    for path in candidates {
        if usable(&path) {
            return Some(path);
        }
    }
    for name in ["python3", "python"] {
        let path = PathBuf::from(name);
        if usable(&path) {
            return Some(path);
        }
    }
    None
}

/// Runs, and reports a version. The Store stub fails this.
fn usable(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map(|out| {
            out.status.success() && String::from_utf8_lossy(&out.stdout).starts_with("Python 3")
        })
        .unwrap_or(false)
}

/// Find the tracer checkout.
///
/// Two places, because there are two ways to be running. In a development
/// tree it is the submodule under `vendor/`. In an installed copy there is no
/// project tree at all — the app was launched from a Start-menu shortcut and
/// the working directory is wherever Windows felt like — so the copy that
/// ships beside the executable is the one that exists.
///
/// Falls back to the development path when neither is present, so the error
/// names somewhere a person can go and look.
pub fn vendored(root: &Path) -> PathBuf {
    let development = root.join("vendor").join("code-visualizer");
    let mut tried = vec![development.clone()];

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Where Tauri puts bundled resources — beside the binary on
            // Windows and for a plain `cargo build` copy, `Contents/Resources`
            // in a Mac's app, `lib/Proton` on Linux — and an older layout.
            tried.push(dir.join("code-visualizer"));
            tried.push(dir.join("resources").join("code-visualizer"));
            tried.push(dir.join("../Resources").join("code-visualizer"));
            tried.push(dir.join("../lib/Proton").join("code-visualizer"));
        }
    }

    tried
        .iter()
        .find(|path| path.join("server").join("app.py").is_file())
        .cloned()
        .unwrap_or(development)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_port_is_actually_free() {
        let port = free_port().unwrap();
        // Bindable again, which is the property the child needs.
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }

    #[test]
    fn the_checkout_is_found_in_a_development_tree() {
        // The submodule, which is where it lives while working on Proton.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let found = vendored(&root);
        if root.join("vendor/code-visualizer/server/app.py").is_file() {
            assert!(found.ends_with("vendor/code-visualizer"), "{found:?}");
        }
    }

    #[test]
    fn an_installed_copy_looks_beside_the_executable() {
        // There is no project tree in an installed copy — the app was started
        // from a Start-menu shortcut and the working directory is wherever
        // Windows felt like. The copy that ships next to the binary is the
        // only one that exists, so the search has to reach it.
        let exe = std::env::current_exe().expect("a test binary has a path");
        let beside = exe.parent().unwrap().join("code-visualizer");
        std::fs::create_dir_all(beside.join("server")).unwrap();
        std::fs::write(beside.join("server").join("app.py"), "# stand-in").unwrap();

        // A root with nothing in it, which is what an installed copy has.
        let empty = std::env::temp_dir().join(format!("proton-empty-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();

        let found = vendored(&empty);
        assert_eq!(
            found, beside,
            "should have found the copy beside the binary"
        );

        std::fs::remove_dir_all(&beside).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn with_no_checkout_anywhere_the_error_names_the_development_path() {
        // Falling back to somewhere a person can go and look beats reporting
        // the last place searched, which is a resources directory they have
        // never heard of.
        let nowhere = std::env::temp_dir().join("proton-no-such-root");
        let found = vendored(&nowhere);
        assert!(found.ends_with("code-visualizer"), "{found:?}");
    }

    #[test]
    fn starting_from_a_directory_with_no_tracer_says_so() {
        // Not "could not start": the message has to name what was missing, or
        // the user checks their Python for an hour.
        let err = Server::start(Path::new(".")).expect_err("no tracer here");
        assert!(err.contains("server/app.py"), "{err}");
    }
}
