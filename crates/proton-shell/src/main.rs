// The window has no OS decorations — the titlebar, traffic lights and mode rail
// are all drawn in the page. Hiding the console subsystem keeps a stray terminal
// from appearing behind it on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backdrop;
mod pasteboard;
mod transport;
mod window;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use backdrop::Backdrop;
use proton_core::{Command, Session};
use transport::{BridgeSession, EventBatch};
use tauri::{Emitter, Manager, State};

/// The channel language servers arrive on. Distinct from the `dispatch` reply
/// path because nothing asked for these: a diagnostic shows up because a
/// server finished thinking, not because the user pressed a key. Each window
/// has its own, `proton://events/<label>`: a page's `listen` hears every
/// window's events whatever they are sent to, so they are told apart by name.
const PUSHED: &str = "proton://events";

/// The channel a window's pushed events go on.
fn channel(label: &str) -> String {
    format!("{PUSHED}/{label}")
}

/// Each window's own core, by the window's label. A window is a Proton of
/// its own — its projects, its modes, its agents and terminals — sharing
/// with the others only what the machine has one of: the Lab, Lean. The
/// first window's session is the one written down and reopened next launch.
#[derive(Default)]
struct Windows {
    sessions: Mutex<Vec<(String, Arc<Mutex<BridgeSession>>)>>,
    /// How many have been opened after the first, for the next one's name.
    opened: AtomicUsize,
    /// What each window's question about unsaved changes was asked for.
    asked: Mutex<HashMap<String, Ending>>,
    /// The windows whose unsaved changes have been answered for, this quit.
    settled: Mutex<HashSet<String>>,
}

/// Why a window asked about its unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// Proton is quitting: every window is asked in turn.
    Quit,
    /// Only this window is closing; the others go on.
    Close,
}

/// A lock that outlives a panic: the previous command's, already reported.
fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Windows {
    fn of(&self, label: &str) -> Option<Arc<Mutex<BridgeSession>>> {
        held(&self.sessions).iter().find(|(l, _)| l == label).map(|(_, s)| Arc::clone(s))
    }

    fn all(&self) -> Vec<(String, Arc<Mutex<BridgeSession>>)> {
        held(&self.sessions).clone()
    }

    fn add(&self, label: &str, session: BridgeSession) {
        held(&self.sessions).push((label.to_string(), Arc::new(Mutex::new(session))));
    }

    fn remove(&self, label: &str) -> Option<Arc<Mutex<BridgeSession>>> {
        let mut sessions = held(&self.sessions);
        let at = sessions.iter().position(|(l, _)| l == label)?;
        Some(sessions.remove(at).1)
    }
}

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
fn dispatch(command: Command, window: tauri::WebviewWindow, windows: State<'_, Windows>) -> EventBatch {
    // A window being let go of has no session left to ask.
    let Some(session) = windows.of(window.label()) else {
        return EventBatch { sequence: 0, events: Vec::new() };
    };
    // A poisoned lock means a previous command panicked. Recover rather than
    // propagating: the front end should keep working, and the panic has
    // already been reported on stderr.
    let events = held(&session).apply(command.clone());
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
/// Only a file the core has shown as a document is read — asked under the
/// session's lock, and read after letting go of it — and never one larger
/// than a document could reasonably be. Off the main thread, which a 40 MB
/// read would otherwise hold, and the window with it.
#[tauri::command(async)]
fn read_document(path: String, window: tauri::WebviewWindow, windows: State<'_, Windows>) -> Result<tauri::ipc::Response, String> {
    /// The largest file read for the page: the library reads none larger.
    const LARGEST: u64 = 128 * 1024 * 1024;
    let file = windows.of(window.label()).and_then(|s| held(&s).session.document_file(&path));
    let file = file.ok_or_else(|| format!("{path}: not a document Proton opened"))?;
    let size = std::fs::metadata(&file).map_err(|err| format!("{path}: {err}"))?.len();
    if size > LARGEST {
        return Err(format!("{path}: {} MB is more than a document is read at", size / (1024 * 1024)));
    }
    let bytes = std::fs::read(&file).map_err(|err| format!("{path}: {err}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// A file a Lab run left — a figure, a 3D scene — as bytes.
///
/// Where it is is asked of the core under the session's lock, which only
/// gives a file inside that run's folder of a kind the Lab shows; it is read
/// after the lock is let go, so a large picture never holds the core.
#[tauri::command(async)]
fn read_lab(key: String, window: tauri::WebviewWindow, windows: State<'_, Windows>) -> Result<tauri::ipc::Response, String> {
    let path = windows.of(window.label()).and_then(|s| held(&s).session.lab_file(&key));
    let path = path.ok_or_else(|| format!("{key}: not a file a run left"))?;
    std::fs::read(&path).map(tauri::ipc::Response::new).map_err(|e| format!("{key}: {e}"))
}

/// Save a copy of the document on screen where the person chooses, through
/// the system's save panel — only a file the viewer was let read, so the page
/// cannot have the shell copy anything else. `None` when they cancel.
#[tauri::command(async)]
fn save_document_as(app: tauri::AppHandle, path: String, window: tauri::WebviewWindow, windows: State<'_, Windows>) -> Result<Option<String>, String> {
    let file = windows.of(window.label()).and_then(|s| held(&s).session.document_file(&path));
    let file = file.ok_or_else(|| format!("{path}: not a document Proton opened"))?;
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("document.pdf").to_string();
    // The panel is AppKit's, so it is shown on the main thread; this waits.
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(save_panel::choose(&name));
    })
    .map_err(|err| err.to_string())?;
    let Some(to) = rx.recv().map_err(|err| err.to_string())? else { return Ok(None) };
    std::fs::copy(&file, &to).map_err(|err| format!("{}: {err}", to.display()))?;
    Ok(Some(to.display().to_string()))
}

/// The system's save panel, asked where a file should go.
mod save_panel {
    use std::path::PathBuf;

    /// Where the person chose to save `name`, or `None` if they cancelled.
    #[cfg(target_os = "macos")]
    pub fn choose(name: &str) -> Option<PathBuf> {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let c = std::ffi::CString::new(name).ok()?;
        unsafe {
            let panel: *mut AnyObject = msg_send![class!(NSSavePanel), savePanel];
            if panel.is_null() {
                return None;
            }
            let ns: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
            let _: () = msg_send![panel, setNameFieldStringValue: ns];
            let _: () = msg_send![panel, setCanCreateDirectories: true];
            // NSModalResponseOK is 1.
            let answer: isize = msg_send![panel, runModal];
            if answer != 1 {
                return None;
            }
            let url: *mut AnyObject = msg_send![panel, URL];
            if url.is_null() {
                return None;
            }
            let path: *mut AnyObject = msg_send![url, path];
            if path.is_null() {
                return None;
            }
            let utf8: *const std::ffi::c_char = msg_send![path, UTF8String];
            if utf8.is_null() {
                return None;
            }
            Some(PathBuf::from(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned()))
        }
    }

    /// Elsewhere, the Downloads folder, under a name not already taken.
    #[cfg(not(target_os = "macos"))]
    pub fn choose(name: &str) -> Option<PathBuf> {
        let downloads = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|h| PathBuf::from(h).join("Downloads"))?;
        let mut to = downloads.join(name);
        let mut n = 2;
        while to.exists() {
            to = downloads.join(format!("{n} {name}"));
            n += 1;
        }
        Some(to)
    }
}

/// ⌘V in the editor: the clipboard's text, read here because a page is given
/// it only when something editable has the focus, and the editor is drawn.
#[tauri::command]
fn paste_text() -> Option<String> {
    pasteboard::text()
}

/// What the editor yanked or deleted, put on the system clipboard as well.
#[tauri::command]
fn copy_text(text: String) {
    pasteboard::put_text(&text);
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
                let Some(polled) = poll_once(&app) else {
                    // The app is shutting down and the state is gone.
                    return;
                };
                for (label, events) in polled {
                    if events.events.is_empty() {
                        continue;
                    }
                    stirred = Instant::now();
                    #[cfg(debug_assertions)]
                    eprintln!("[poll] {label}: {} event(s): {events:?}", events.events.len());
                    if app.emit(&channel(&label), events).is_err() {
                        return;
                    }
                }
            }
        })
        .expect("the poll thread is not optional: without it no server is ever heard from");
}

/// One poll of every window's core, or `None` once the app is going away.
fn poll_once(app: &tauri::AppHandle) -> Option<Vec<(String, EventBatch)>> {
    let windows = app.try_state::<Windows>()?;
    Some(windows.all().into_iter().map(|(label, session)| {
        let events = held(&session).apply(Command::Poll);
        (label, events)
    }).collect())
}


/// Set once quitting is settled — nothing to lose, or the person said to go
/// on — so it is not asked about again.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// The menu's Quit, which asks first: see `menu_that_asks`.
const QUIT: &str = "proton-quit";

/// Proton is to quit: whether to stop and ask first. When quitting would
/// lose unsaved changes, the first window holding some is told which files,
/// and the quit waits for its answer — then asks the next.
fn asks_first(app: &tauri::AppHandle) -> bool {
    if QUITTING.load(Ordering::SeqCst) {
        return false;
    }
    if let Some(windows) = app.try_state::<Windows>() {
        held(&windows.settled).clear();
    }
    asks_next(app)
}

/// Ask the next window with unsaved changes not yet answered for, this quit.
fn asks_next(app: &tauri::AppHandle) -> bool {
    let Some(windows) = app.try_state::<Windows>() else {
        return false;
    };
    let settled = held(&windows.settled).clone();
    for (label, session) in windows.all() {
        if settled.contains(&label) {
            continue;
        }
        let batch = held(&session).apply(Command::AskToQuit);
        if batch.events.is_empty() {
            continue;
        }
        held(&windows.asked).insert(label.clone(), Ending::Quit);
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        let _ = app.emit(&channel(&label), batch);
        return true;
    }
    false
}

/// One window is to close, the others staying: whether to stop and ask it
/// about its own unsaved changes first.
fn asks_to_close(app: &tauri::AppHandle, label: &str) -> bool {
    let Some(windows) = app.try_state::<Windows>() else {
        return false;
    };
    let Some(session) = windows.of(label) else {
        return false;
    };
    let batch = held(&session).apply(Command::AskToClose);
    if batch.events.is_empty() {
        return false;
    }
    held(&windows.asked).insert(label.to_string(), Ending::Close);
    let _ = app.emit(&channel(label), batch);
    true
}

/// The window's answer: its files saved, when asked to, and refused, saying
/// which could not be and why, when one could not; then the window closed,
/// if only it was closing, or the next window asked, or Proton quit.
#[tauri::command]
fn quit(save: bool, window: tauri::WebviewWindow, app: tauri::AppHandle, windows: State<'_, Windows>) -> Result<(), String> {
    let label = window.label().to_string();
    if save {
        if let Some(session) = windows.of(&label) {
            let failed = held(&session).session.save_unsaved();
            if !failed.is_empty() {
                return Err(failed.join("\n"));
            }
        }
    }
    let ending = held(&windows.asked).remove(&label);
    if ending == Some(Ending::Close) {
        // Not asked again on the way out: destroyed, not closed.
        return window.destroy().map_err(|err| err.to_string());
    }
    held(&windows.settled).insert(label);
    if asks_next(&app) {
        return Ok(());
    }
    QUITTING.store(true, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

/// A window gone: its session written down (if it is the one that is) and
/// what it had running stopped — all of Proton's, if it was the last.
fn let_go(app: &tauri::AppHandle, label: &str) {
    let Some(windows) = app.try_state::<Windows>() else {
        return;
    };
    held(&windows.asked).remove(label);
    let Some(session) = windows.remove(label) else {
        return;
    };
    let last = windows.all().is_empty();
    let mut session = held(&session);
    session.session.save();
    if last {
        session.session.shutdown();
    } else {
        session.session.shutdown_window();
    }
}

/// What every window does when closed: asks about its unsaved changes —
/// the whole quit's, if it is the last one — and lets its session go.
fn watch(window: &tauri::WebviewWindow) {
    let this = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            let app = this.app_handle();
            let others = app.webview_windows().keys().any(|label| label != this.label());
            let asks = if others { asks_to_close(app, this.label()) } else { asks_first(app) };
            if asks {
                api.prevent_close();
            }
        }
        tauri::WindowEvent::Destroyed => let_go(this.app_handle(), this.label()),
        _ => {}
    });
}

/// The menu's New Window.
const NEW_WINDOW: &str = "proton-new-window";

/// Another window, with a session of its own on the home folder: in front of
/// the one in front, as large, a little down and to the right.
fn open_window(app: &tauri::AppHandle) -> Result<(), String> {
    let windows = app.try_state::<Windows>().ok_or("Proton is closing")?;
    let label = format!("window-{}", windows.opened.fetch_add(1, Ordering::SeqCst) + 2);
    // Before the window, so its page's first command finds it.
    windows.add(&label, BridgeSession::new(Session::another_window()));
    let front = app.webview_windows().into_values().find(|w| w.is_focused().unwrap_or(false)).or_else(|| app.get_webview_window("main"));
    let mut builder = tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::App("index.html".into()))
        .title("Proton")
        .inner_size(1600.0, 1000.0)
        .min_inner_size(1100.0, 700.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .theme(Some(tauri::Theme::Dark))
        .disable_drag_drop_handler()
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Throttle);
    if let Some(front) = &front {
        let scale = front.scale_factor().unwrap_or(1.0);
        if let Ok(size) = front.inner_size() {
            let size = size.to_logical::<f64>(scale);
            builder = builder.inner_size(size.width, size.height);
        }
        if let Ok(at) = front.outer_position() {
            let at = at.to_logical::<f64>(scale);
            builder = builder.position(at.x + 28.0, at.y + 28.0);
        }
    }
    let window = match builder.build() {
        Ok(window) => window,
        Err(err) => {
            if let Some(session) = windows.remove(&label) {
                held(&session).session.shutdown_window();
            }
            return Err(err.to_string());
        }
    };
    dress(&window, app);
    watch(&window);
    let _ = window.set_focus();
    Ok(())
}

/// A window made to look like the first: its icon, the backdrop in use, the
/// page kept drawn while out of sight.
fn dress(window: &tauri::WebviewWindow, app: &tauri::AppHandle) {
    if let Some(icon) = window_icon() {
        // A failed icon is cosmetic; it must not stop the window opening.
        if let Err(err) = window.set_icon(icon) {
            eprintln!("proton: could not set the window icon: {err}");
        }
    }
    let material = app.try_state::<Surface>().map(|s| *held(&s)).unwrap_or_default();
    backdrop::apply(window, material);
    keep_drawing(window);
}

/// The default menu, with a Quit that asks first and a New Window.
///
/// The menu's own Quit ends the app before anything can ask: tao answers
/// macOS's `applicationWillTerminate` but not `applicationShouldTerminate`,
/// the one that can say no. So ⌘Q is given to an item of ours instead.
#[cfg(target_os = "macos")]
fn menu_that_asks(app: &tauri::AppHandle) -> Option<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind};
    let menu = Menu::default(app).ok()?;
    let mut quit_given = false;
    for item in menu.items().ok()? {
        let MenuItemKind::Submenu(submenu) = item else { continue };
        // File's first item: ⌘⇧N, as in every editor that has windows.
        if submenu.text().is_ok_and(|text| text == "File") {
            let new = MenuItem::with_id(app, NEW_WINDOW, "New Window", true, Some("CmdOrCtrl+Shift+N")).ok()?;
            submenu.insert(&new, 0).ok()?;
        }
        for (at, entry) in submenu.items().ok()?.into_iter().enumerate() {
            let MenuItemKind::Predefined(predefined) = &entry else { continue };
            if !predefined.text().is_ok_and(|text| text.starts_with("Quit")) {
                continue;
            }
            let quit = MenuItem::with_id(app, QUIT, "Quit Proton", true, Some("CmdOrCtrl+Q")).ok()?;
            submenu.remove(predefined).ok()?;
            submenu.insert(&quit, at).ok()?;
            quit_given = true;
        }
    }
    quit_given.then_some(menu)
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

/// Keep the page drawn while the window is out of sight.
///
/// WebKit counts a window covered by others, or on another desktop, as not
/// visible, and marks what the page drew as memory the system may take back.
/// Under memory pressure it is taken, and Mission Control — which shows the
/// other desktops from what each window last drew — shows only the frosted
/// backdrop. `backgroundThrottling: "throttle"` (tauri.conf.json) stopped
/// WebKit suspending the page, but not this; it came back on a day of builds
/// that filled the swap. Told not to watch the window's occlusion, WebKit
/// keeps the page visible until the window is minimised or Proton hidden. The
/// cost is a covered page whose timers run at their own speed, which for
/// Proton at rest is the caret's blink. A private call — the app already uses
/// macOS's private API — asked for only where this WebKit has it.
fn keep_drawing(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let done = window.with_webview(|webview| {
            use objc2::runtime::AnyObject;
            use objc2::{msg_send, sel};
            let view = webview.inner() as *mut AnyObject;
            // SAFETY: `view` is this window's live WKWebView, and Tauri runs
            // this on the main thread.
            unsafe {
                let has: bool = msg_send![view, respondsToSelector: sel!(_setWindowOcclusionDetectionEnabled:)];
                if has {
                    let _: () = msg_send![view, _setWindowOcclusionDetectionEnabled: false];
                }
            }
        });
        if let Err(err) = done {
            eprintln!("proton: could not keep the page drawn while hidden: {err}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

fn main() {
    // Opened from the Finder or the Dock, this has launchd's PATH, and so
    // would everything it starts by name — ffprobe, pandoc, git, a kernel's
    // `!pip`. What a login shell adds is added first, while this is still the
    // only thread: setting the environment races with anything reading it.
    if let Some(path) = proton_claude::process::launch_path() {
        std::env::set_var("PATH", path);
    }
    // The interactive login shell's PATH — conda's, nvm's — which the engines
    // and a notebook's kernel start with: asked now and off this thread, so
    // neither the first chat nor the first cell waits seconds for it.
    std::thread::spawn(|| {
        proton_claude::process::login_path();
        // The PATH kept from the last launch, asked for again for the next.
        proton_claude::process::refresh_launch_path();
    });
    let windows = Windows::default();
    windows.add("main", BridgeSession::new(Session::restore(requested_folder(), working_folder())));
    tauri::Builder::default()
        .manage(windows)
        .invoke_handler(tauri::generate_handler![
            dispatch,
            backdrop,
            set_backdrop,
            set_opaque,
            read_document,
            read_lab,
            save_document_as,
            paste_text,
            copy_text,
            quit
        ])
        .setup(|app| {
            let Some(window) = app.get_webview_window("main") else {
                // No window means the config is wrong, not that we should limp
                // on with a half-configured app.
                return Err("no main window in tauri.conf.json".into());
            };

            // Before anything else touches the window: it opens where it was
            // last closed, and the size in `tauri.conf.json` is only the
            // first-run default. Only the first window's place is kept.
            window::restore(&window);
            window::remember(&window);

            let applied = backdrop::apply(&window, Backdrop::default());
            println!("[shell] backdrop: {applied}");
            app.manage(Surface::new(applied));
            dress(&window, app.handle());

            spawn_poll(app.handle().clone());

            // Quitting with unsaved changes asks first — from the window's
            // close, which the page draws, and from ⌘Q.
            #[cfg(target_os = "macos")]
            if let Some(menu) = menu_that_asks(app.handle()) {
                let _ = app.set_menu(menu);
            }
            app.on_menu_event(|app, event| {
                if event.id() == QUIT && !asks_first(app) {
                    QUITTING.store(true, Ordering::SeqCst);
                    app.exit(0);
                }
                if event.id() == NEW_WINDOW {
                    // Off the main thread: a session starts its servers.
                    let app = app.clone();
                    std::thread::spawn(move || {
                        if let Err(err) = open_window(&app) {
                            eprintln!("proton: could not open another window: {err}");
                        }
                    });
                }
            });
            watch(&window);

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Proton failed to start")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = &event {
                if asks_first(app) {
                    api.prevent_exit();
                }
            }
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(windows) = app.try_state::<Windows>() {
                    for (label, session) in windows.all() {
                        windows.remove(&label);
                        let mut session = held(&session);
                        session.session.save();
                        // A Lab run and a notebook's kernel each lead a
                        // process group of their own, and the app exits
                        // without dropping the session: they would outlive
                        // it if not stopped here.
                        session.session.shutdown();
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn each_window_has_a_session_of_its_own_until_it_is_let_go() {
        let windows = Windows::default();
        let at = |dir: &str| BridgeSession::new(Session::new(std::env::temp_dir().join(dir)));
        windows.add("main", at("proton-shell-main"));
        windows.add("window-2", at("proton-shell-second"));
        let main = windows.of("main").unwrap();
        let second = windows.of("window-2").unwrap();
        assert!(!Arc::ptr_eq(&main, &second), "two windows share a core");
        assert!(held(&second).session.active().root().ends_with("proton-shell-second"));
        assert!(windows.of("window-3").is_none());
        assert!(windows.remove("window-2").is_some());
        assert!(windows.of("window-2").is_none() && windows.remove("window-2").is_none());
        assert_eq!(windows.all().iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["main"]);
        // Pushes go to the window they are for, by name.
        assert_eq!(channel("window-2"), "proton://events/window-2");
    }

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
