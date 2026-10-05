// The window has no OS decorations — the titlebar, traffic lights and mode rail
// are all drawn in the page. Hiding the console subsystem keeps a stray terminal
// from appearing behind it on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backdrop;
mod transport;
mod window;

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use backdrop::Backdrop;
use proton_core::{Command, Session};
use transport::{BridgeSession, EventBatch};
use tauri::{Emitter, Manager, State};

/// The channel language servers arrive on. Distinct from the `dispatch` reply
/// path because nothing asked for these: a diagnostic shows up because a
/// server finished thinking, not because the user pressed a key.
const PUSHED: &str = "proton://events";

/// How often to ask the core whether anything has happened on a thread it
/// cannot see — a server, a shell, the agent.
///
/// The core has no clock and no threads — that is what makes it testable — so
/// something has to ask on its behalf, and this is that something. It used to
/// ask twenty times a second whatever was going on, which is twenty wake-ups a
/// second for a window nobody is using and a twentieth of a second added to
/// every echoed keystroke in the terminal. Now it keeps time with the work:
/// once a frame while things are happening, slower as they stop, and rarely
/// once they have — and it is woken the moment a command arrives or a shell
/// writes, so being asleep never makes anything late.
const FAST: Duration = Duration::from_millis(16);
/// For a few seconds after things go quiet: a language server answering a
/// save, an agent thinking before its first word.
const BEAT: Duration = Duration::from_millis(50);
/// Once nothing has happened for a while.
const IDLE: Duration = Duration::from_millis(250);

/// The heartbeat's alarm clock: sleep until the next beat, or until woken.
struct Heartbeat {
    woken: Mutex<bool>,
    bell: Condvar,
}

impl Heartbeat {
    const fn new() -> Heartbeat {
        Heartbeat {
            woken: Mutex::new(false),
            bell: Condvar::new(),
        }
    }

    /// Something happened: poll now rather than at the next beat.
    fn wake(&self) {
        if let Ok(mut woken) = self.woken.lock() {
            *woken = true;
            self.bell.notify_one();
        }
    }

    /// Sleep for `wait`, or less if woken. `true` if it was woken.
    fn sleep(&self, wait: Duration) -> bool {
        let Ok(mut woken) = self.woken.lock() else {
            std::thread::sleep(wait);
            return false;
        };
        if !*woken {
            woken = match self.bell.wait_timeout(woken, wait) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        std::mem::replace(&mut *woken, false)
    }
}

static HEARTBEAT: Heartbeat = Heartbeat::new();

/// How long to sleep, given how long it has been since anything happened.
fn cadence(quiet: Duration) -> Duration {
    if quiet < Duration::from_secs(1) {
        FAST
    } else if quiet < Duration::from_secs(5) {
        BEAT
    } else {
        IDLE
    }
}

/// What the compositor actually gave us. The front end needs this to decide
/// whether to paint its own ground, so it reads it on boot and after any change.
type Surface = Mutex<Backdrop>;

/// The whole bridge. One command in, the events it produced out.
///
/// Work that finishes later — agent deltas, trace steps, LSP diagnostics — is
/// pushed by the heartbeat through `AppHandle::emit` instead; the `Event` type
/// is the same shape for both, which is the point of keeping it transport-
/// agnostic.
///
/// Off the main thread. A command Tauri is not told is `async` runs on the
/// thread that runs the window, so every keystroke's work — and every wait for
/// the session's lock while the heartbeat held it — was time the window could
/// not be moved, resized or redrawn in. The page sends one command at a time
/// and waits for each reply, so running them elsewhere cannot reorder them.
#[tauri::command(async)]
fn dispatch(command: Command, session: State<'_, Mutex<BridgeSession>>) -> EventBatch {
    let events = match session.lock() {
        Ok(mut it) => it.apply(command.clone()),
        // A poisoned lock means a previous command panicked. Recover rather
        // than propagating: the front end should keep working, and the panic
        // has already been reported on stderr.
        Err(poisoned) => poisoned.into_inner().apply(command.clone()),
    };
    // Whatever was just asked for may answer on another thread — a shell
    // echoing the key, a server replying — so the heartbeat looks sooner.
    HEARTBEAT.wake();

    // A dropped keystroke is the worst failure mode a modal editor has, and it
    // is invisible from the page. Tracing the seam in debug builds makes it
    // visible — and is how M0's exit criterion gets verified without a
    // screenshot.
    #[cfg(debug_assertions)]
    eprintln!(
        "[bridge] {command:?} -> {} event(s): {events:?}",
        events.events.len()
    );

    events
}

/// Which backdrop is live: `mica`, `mica-alt`, `acrylic`, `vibrancy`, `solid`.
#[tauri::command]
fn backdrop(surface: State<'_, Surface>) -> String {
    surface
        .lock()
        .map(|b| b.to_string())
        .unwrap_or_else(|_| Backdrop::Solid.to_string())
}

/// Switch materials at runtime, and report what was actually applied — which
/// may not be what was asked for.
#[tauri::command]
fn set_backdrop(
    material: Backdrop,
    window: tauri::WebviewWindow,
    surface: State<'_, Surface>,
) -> String {
    let applied = backdrop::apply(&window, material);
    if let Ok(mut current) = surface.lock() {
        *current = applied;
    }
    applied.to_string()
}

/// Make the window opaque in the given `#rrggbb`, or see-through again with
/// the chosen material behind it — Codex asks for the first, the modes drawn
/// as glass for the second. Not `async`: AppKit wants the main thread.
#[tauri::command]
fn set_opaque(color: Option<String>, window: tauri::WebviewWindow, surface: State<'_, Surface>) {
    let material = surface.lock().map(|current| *current).unwrap_or_default();
    let color = color.as_deref().map(|hex| backdrop::rgb(hex).unwrap_or((0.0, 0.0, 0.0)));
    backdrop::set_opaque(&window, color, material);
}

/// A project the user actually named: `proton .` or `proton C:\src\thing`, the
/// way every editor takes it.
///
/// `None` means nothing was named, which is not the same as "the current
/// directory" — see [`working_folder`].
fn requested_folder() -> Option<std::path::PathBuf> {
    let given = std::env::args_os().nth(1)?;
    let path = std::path::PathBuf::from(given);
    path.is_dir().then_some(path)
}

/// The working directory, when it is worth anything.
///
/// Only a fallback. It is a reasonable guess on a first run from a terminal and
/// meaningless afterwards, so the session uses it only when there is nothing to
/// restore and nothing was named — otherwise a bare `proton` typed in a
/// downloads folder would add it to the projects list for good.
///
/// `None` in the install folder or at the top of a disk — see
/// [`could_be_a_project`] — and the session then starts in the home folder.
fn working_folder() -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let exe = std::env::current_exe().ok();
    could_be_a_project(&cwd, exe.as_deref().and_then(std::path::Path::parent)).then_some(cwd)
}

/// Whether a process started in `cwd`, from an executable in `exe_folder`, was
/// started somewhere that could be a project.
///
/// Not the install folder, which is where a Start-menu shortcut starts it, and
/// indexing that would be nonsense. Nor the top of a disk, which is where a Mac
/// app opened from the Dock, the Finder or Spotlight starts: as a project that
/// is every file on the machine, walked by the index and then written into the
/// session to be reopened at every launch.
///
/// A drive's root on Windows — `C:\`, or a share's `\\server\share\` — is the
/// top of a disk as much as `/` is, and is turned away the same way. A project
/// that really does fill a drive, a `subst` one say, can still be named:
/// `proton .`.
fn could_be_a_project(cwd: &std::path::Path, exe_folder: Option<&std::path::Path>) -> bool {
    let beside_the_exe = exe_folder.is_some_and(|dir| dir == cwd);
    // Nothing above it: `/`, `C:\`, `\\server\share\`.
    let top_of_a_disk = cwd.parent().is_none();
    !beside_the_exe && !top_of_a_disk
}

/// The bytes of a file the page renders itself.
///
/// Raw, through `ipc::Response`, which hands the webview an ArrayBuffer. The
/// two alternatives are both worse: base64 through the JSON channel inflates a
/// PDF by a third and makes every other event queue behind it, and Tauri's
/// asset protocol would mean enabling a second scheme and choosing a path
/// scope for it.
///
/// This grants the page no reach it did not already have. It runs only
/// first-party code — the CSP forbids remote scripts — and it can already ask
/// the core to open any file by name and read the text back out of the
/// viewport events. What is new here is the encoding, not the access.
#[tauri::command]
fn read_document(path: String) -> Result<tauri::ipc::Response, String> {
    let bytes = std::fs::read(&path).map_err(|err| format!("{path}: {err}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// One mesh, as bytes.
///
/// The binary half of `Event::Geometry`: the event says what a mesh is and
/// whether it is ready, and this sends the mesh itself through `ipc::Response`,
/// the same way `read_document` sends a PDF. As JSON a large mesh cost more to
/// write out in Rust and parse back in the page than it did to move; as bytes
/// the page lays typed arrays straight over the buffer it is handed.
///
/// The key is a string the page chose, so it goes through the same door the
/// event did — `Modeling::mesh_bytes` refuses a path that climbs out of the
/// scene's folder exactly as loading one does. Nothing new is reachable here.
#[tauri::command]
fn read_geometry(
    key: String,
    session: State<'_, Mutex<BridgeSession>>,
) -> Result<tauri::ipc::Response, String> {
    let bytes = match session.lock() {
        Ok(mut it) => it.session.mesh_bytes(&key),
        Err(poisoned) => poisoned.into_inner().session.mesh_bytes(&key),
    };
    bytes
        .map(tauri::ipc::Response::new)
        .ok_or_else(|| format!("{key}: no geometry to send"))
}

/// A file a Lab run left — a figure, a 3D scene — as bytes.
///
/// Where it is is asked of the core under the session's lock, which only
/// gives a file inside that run's folder of a kind the Lab shows; it is read
/// after the lock is let go, so a large picture never holds the core.
#[tauri::command]
fn read_lab(key: String, session: State<'_, Mutex<BridgeSession>>) -> Result<tauri::ipc::Response, String> {
    let path = match session.lock() {
        Ok(it) => it.session.lab_file(&key),
        Err(poisoned) => poisoned.into_inner().session.lab_file(&key),
    };
    let path = path.ok_or_else(|| format!("{key}: not a file a run left"))?;
    std::fs::read(&path).map(tauri::ipc::Response::new).map_err(|e| format!("{key}: {e}"))
}

/// Give the core a heartbeat.
///
/// Everything a language server says arrives on a thread the core cannot see,
/// so the mailbox has to be drained by someone with a clock. This is the whole
/// of that arrangement: ask, and if anything came back, push it at the page.
/// Nothing is emitted when nothing happened, which is the common case by
/// several orders of magnitude.
fn spawn_poll(app: tauri::AppHandle) {
    // A shell writing is the one thing worth waking for that no command asked
    // for just before: a build's output, a prompt coming back.
    proton_core::on_terminal_output(|| HEARTBEAT.wake());
    // Claude Code streaming a reply is the same shape: a thread the core
    // cannot see, writing a line at a time.
    proton_core::on_claude_output(|| HEARTBEAT.wake());
    std::thread::Builder::new()
        .name("proton-poll".into())
        .spawn(move || {
            let mut stirred = Instant::now();
            let mut last = Instant::now();
            loop {
                if HEARTBEAT.sleep(cadence(stirred.elapsed())) {
                    stirred = Instant::now();
                }
                // Never more than once a frame, however often it is woken: a
                // shell printing a log writes thousands of times a second.
                let since = last.elapsed();
                if since < FAST {
                    std::thread::sleep(FAST - since);
                }
                last = Instant::now();
                if let Some(events) = poll_once(&app) {
                    if events.events.is_empty() {
                        continue;
                    }
                    stirred = Instant::now();
                    #[cfg(debug_assertions)]
                    eprintln!("[poll] {} event(s): {events:?}", events.events.len());
                    if app.emit(PUSHED, events).is_err() {
                        return;
                    }
                } else {
                    // The app is shutting down and the state is gone.
                    return;
                }
            }
        })
        .expect("the poll thread is not optional: without it no server is ever heard from");
}

/// One poll of the core, or `None` once the app is going away.
fn poll_once(app: &tauri::AppHandle) -> Option<EventBatch> {
    let session = app.try_state::<Mutex<BridgeSession>>()?;
    let events = match session.lock() {
        Ok(mut it) => it.apply(Command::Poll),
        Err(poisoned) => poisoned.into_inner().apply(Command::Poll),
    };
    Some(events)
}


/// The window and taskbar icon.
///
/// Pinned to the 256px raster explicitly rather than left to the bundle list:
/// the generated context picks the first entry, which is the 32px one, and on a
/// high-DPI display Windows upscales that into something visibly soft.
fn window_icon() -> Option<tauri::image::Image<'static>> {
    match tauri::image::Image::from_bytes(include_bytes!("../icons/256x256.png")) {
        Ok(image) => Some(image),
        Err(err) => {
            eprintln!("proton: could not decode the window icon: {err}");
            None
        }
    }
}

/// An artifact page, from the copies Proton keeps of what Claude Code
/// published.
///
/// `artifact://localhost/<id>/<version>/<file>` and nothing else: the path is
/// resolved by `proton_claude::artifacts::resolve`, which answers only with a
/// file inside the artifact folder. Every page is served under the policy
/// artifacts are written for — inline script, the CDNs claude.ai allows — and
/// that policy names neither `ipc:` nor the editor's own origin, so a page
/// can draw itself and reach nothing of Proton's. The pane frames it
/// sandboxed besides, with an origin of its own.
fn artifact_page(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    use proton_claude::artifacts;
    let root = artifacts::root();
    let found = artifacts::resolve(&root, request.uri().path()).and_then(|file| {
        let bytes = std::fs::read(&file).ok()?;
        Some((artifacts::content_type(&file), bytes))
    });
    let builder = tauri::http::Response::builder()
        .header("Content-Security-Policy", artifacts::CSP)
        .header("X-Content-Type-Options", "nosniff")
        // A sandboxed frame's origin is opaque, so a page fetching its own
        // data file is a cross-origin request to itself.
        .header("Access-Control-Allow-Origin", "*");
    let response = match found {
        Some((kind, bytes)) => builder.status(200).header("Content-Type", kind).body(bytes),
        None => builder
            .status(404)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(b"This artifact is not kept here.".to_vec()),
    };
    response.unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

fn main() {
    tauri::Builder::default()
        .register_uri_scheme_protocol("artifact", |_, request| artifact_page(&request))
        .manage(Mutex::new(BridgeSession::new(Session::restore(
            requested_folder(),
            working_folder(),
        ))))
        .invoke_handler(tauri::generate_handler![
            dispatch,
            backdrop,
            set_backdrop,
            set_opaque,
            read_document,
            read_geometry,
            read_lab
        ])
        .setup(|app| {
            let Some(window) = app.get_webview_window("main") else {
                // No window means the config is wrong, not that we should limp
                // on with a half-configured app.
                return Err("no main window in tauri.conf.json".into());
            };

            if let Some(icon) = window_icon() {
                // A failed icon is cosmetic; it must not stop the app booting.
                if let Err(err) = window.set_icon(icon) {
                    eprintln!("proton: could not set the window icon: {err}");
                }
            }

            // Before anything else touches the window: it opens where it was
            // last closed, and the size in `tauri.conf.json` is only the
            // first-run default.
            window::restore(&window);
            window::remember(&window);

            let applied = backdrop::apply(&window, Backdrop::default());
            println!("[shell] backdrop: {applied}");
            app.manage(Surface::new(applied));

            spawn_poll(app.handle().clone());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Proton failed to start")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(state) = app.try_state::<Mutex<BridgeSession>>() {
                    let mut session = match state.lock() {
                        Ok(session) => session,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    session.session.save();
                    // A Lab run is a process group of its own: it would
                    // outlive the app if it were not stopped here.
                    session.session.shutdown();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_heartbeat_slows_as_things_go_quiet() {
        assert_eq!(cadence(Duration::ZERO), FAST);
        assert_eq!(cadence(Duration::from_secs(2)), BEAT);
        assert_eq!(cadence(Duration::from_secs(60)), IDLE);
    }

    #[test]
    fn a_wake_ends_the_sleep_at_once_and_is_used_up() {
        let beat = std::sync::Arc::new(Heartbeat::new());
        let waker = std::sync::Arc::clone(&beat);
        let started = Instant::now();
        let ringer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            waker.wake();
        });
        assert!(beat.sleep(Duration::from_secs(10)), "slept through the wake");
        assert!(started.elapsed() < Duration::from_secs(2));
        ringer.join().unwrap();
        // Woken once, so the next sleep is a whole one.
        assert!(!beat.sleep(Duration::from_millis(30)));
    }

    #[test]
    fn where_a_terminal_was_is_a_guess_and_the_install_folder_is_not() {
        let exe = Path::new("/opt/proton/bin");
        let project = Path::new("/home/me/src/thing");
        assert!(could_be_a_project(project, Some(exe)));
        // Still a guess when the executable cannot say where it is.
        assert!(could_be_a_project(project, None));
        // A Start-menu shortcut starts it in the folder it was installed to.
        assert!(!could_be_a_project(exe, Some(exe)));
    }

    #[test]
    fn the_top_of_the_disk_is_no_guess_at_a_project() {
        // A Mac app opened from the Dock, the Finder or Spotlight starts in
        // `/`. Taken as a guess, the first workspace would be rooted there:
        // every file on the machine indexed, and the whole disk written down
        // to be reopened at every launch after.
        let exe = Path::new("/Applications/Proton.app/Contents/MacOS");
        assert!(!could_be_a_project(Path::new("/"), Some(exe)));
        assert!(!could_be_a_project(Path::new("/"), None));

        // Only the top itself: a checkout at `/src` is somebody's choice.
        assert!(could_be_a_project(Path::new("/src"), Some(exe)));

        // A drive's root on Windows is the top of a disk as much as `/` is,
        // and so is a share's.
        #[cfg(windows)]
        for top in [r"C:\", r"D:\", r"\\server\share\"] {
            assert!(!could_be_a_project(Path::new(top), Some(exe)), "{top}");
        }
    }
}
