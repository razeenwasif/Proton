// The window has no OS decorations — the titlebar, traffic lights and mode rail
// are all drawn in the page. Hiding the console subsystem keeps a stray terminal
// from appearing behind it on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backdrop;
mod fullscreen;
mod pasteboard;
mod remote_client;
mod remote_protocol;
mod remote_server;
mod transport;
mod window;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use backdrop::Backdrop;
use proton_core::{Command, Session};
use remote_client::RemoteClient;
use remote_protocol::{RemoteCatalog, RemoteDirEntry, RemoteStatus};
use transport::{cadence, held, BridgeSession, EventBatch, Heartbeat, FAST};
use tauri::{Emitter, Manager, State};

/// The channel language servers arrive on. Distinct from the `dispatch` reply
/// path because nothing asked for these: a diagnostic shows up because a
/// server finished thinking, not because the user pressed a key. Each window
/// has its own, `proton://events/<label>`: a page's `listen` hears every
/// window's events whatever they are sent to, so they are told apart by name.
const PUSHED: &str = "proton://events";
const REMOTE_STATUS_EVENT: &str = "proton://remote-status";
const OPEN_REMOTE_EVENT: &str = "proton://open-remote";

/// The channel a window's pushed events go on.
fn channel(label: &str) -> String {
    format!("{PUSHED}/{label}")
}

fn remote_status_channel(label: &str) -> String {
    format!("{REMOTE_STATUS_EVENT}/{label}")
}

fn open_remote_channel(label: &str) -> String {
    format!("{OPEN_REMOTE_EVENT}/{label}")
}

/// Each window's session: either an in-process local [`BridgeSession`] or a
/// [`RemoteClient`] streaming NDJSON over SSH from a remote machine.
enum WindowSession {
    Local(Mutex<BridgeSession>),
    Remote(Arc<RemoteClient>),
}

impl WindowSession {
    fn new_local(session: Session) -> Self {
        Self::Local(Mutex::new(BridgeSession::new(session)))
    }

    fn apply(&self, command: Command) -> Result<EventBatch, String> {
        match self {
            Self::Local(local) => Ok(held(local).apply(command)),
            Self::Remote(remote) => remote.dispatch(command),
        }
    }

    fn is_remote(&self) -> bool {
        matches!(self, Self::Remote(_))
    }

    fn poll_local(&self) -> Option<EventBatch> {
        match self {
            Self::Local(local) => Some(held(local).apply(Command::Poll)),
            Self::Remote(_) => None,
        }
    }

    fn remote_status(&self) -> RemoteStatus {
        match self {
            Self::Local(_) => RemoteStatus::default(),
            Self::Remote(remote) => remote.status(),
        }
    }

    fn save_unsaved(&self) -> Result<Vec<String>, String> {
        match self {
            Self::Local(local) => Ok(held(local).session.save_unsaved()),
            Self::Remote(remote) => remote.save_unsaved(),
        }
    }

    fn read_document_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        const LARGEST: u64 = 128 * 1024 * 1024;
        match self {
            Self::Local(local) => {
                let file = held(local)
                    .session
                    .document_file(path)
                    .ok_or_else(|| format!("{path}: not a document Proton opened"))?;
                let size = std::fs::metadata(&file)
                    .map_err(|err| format!("{path}: {err}"))?
                    .len();
                if size > LARGEST {
                    return Err(format!(
                        "{path}: {} MB is more than a document is read at",
                        size / (1024 * 1024)
                    ));
                }
                std::fs::read(&file).map_err(|err| format!("{path}: {err}"))
            }
            Self::Remote(remote) => remote.read_document(path.to_string()),
        }
    }

    fn document_name(&self, path: &str) -> Result<String, String> {
        match self {
            Self::Local(local) => {
                let file = held(local)
                    .session
                    .document_file(path)
                    .ok_or_else(|| format!("{path}: not a document Proton opened"))?;
                Ok(file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("document.pdf")
                    .to_string())
            }
            Self::Remote(_) => Ok(Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("document.pdf")
                .to_string()),
        }
    }

    fn forward_port(&self, local_port: u16, remote_port: u16) -> Result<RemoteStatus, String> {
        match self {
            Self::Local(_) => Err("SSH port forwarding requires an active remote SSH session".into()),
            Self::Remote(remote) => remote.forward_port(local_port, remote_port),
        }
    }

    fn shutdown(&self, save_local: bool, is_last: bool) {
        match self {
            Self::Local(local) => {
                let mut guard = held(local);
                if save_local {
                    guard.session.save();
                }
                if is_last {
                    guard.session.shutdown();
                } else {
                    guard.session.shutdown_window();
                }
            }
            Self::Remote(remote) => remote.shutdown(),
        }
    }
}

/// Each window's own core, by the window's label. A window is a Proton of
/// its own — its projects, its modes, its agents and terminals — sharing
/// with the others only what the machine has one of. The
/// first window's session is the one written down and reopened next launch.
#[derive(Default)]
struct Windows {
    sessions: Mutex<Vec<(String, Arc<WindowSession>)>>,
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

impl Windows {
    fn of(&self, label: &str) -> Option<Arc<WindowSession>> {
        held(&self.sessions)
            .iter()
            .find(|(l, _)| l == label)
            .map(|(_, s)| Arc::clone(s))
    }

    fn all(&self) -> Vec<(String, Arc<WindowSession>)> {
        held(&self.sessions).clone()
    }

    fn add(&self, label: &str, session: WindowSession) {
        held(&self.sessions).push((label.to_string(), Arc::new(session)));
    }

    fn replace(&self, label: &str, session: WindowSession) -> Option<Arc<WindowSession>> {
        let mut sessions = held(&self.sessions);
        if let Some((_, slot)) = sessions.iter_mut().find(|(l, _)| l == label) {
            let new = Arc::new(session);
            return Some(std::mem::replace(slot, new));
        }
        sessions.push((label.to_string(), Arc::new(session)));
        None
    }

    fn remove(&self, label: &str) -> Option<Arc<WindowSession>> {
        let mut sessions = held(&self.sessions);
        let at = sessions.iter().position(|(l, _)| l == label)?;
        Some(sessions.remove(at).1)
    }
}

static HEARTBEAT: Heartbeat = Heartbeat::new();

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
fn dispatch(
    command: Command,
    window: tauri::WebviewWindow,
    windows: State<'_, Windows>,
) -> Result<EventBatch, String> {
    // A window being let go of has no session left to ask.
    let Some(session) = windows.of(window.label()) else {
        return Ok(EventBatch {
            sequence: 0,
            events: Vec::new(),
        });
    };
    let events = session.apply(command.clone())?;
    // Whatever was just asked for may answer on another thread — a shell
    // echoing the key, a server replying — so the heartbeat looks sooner.
    if !session.is_remote() {
        HEARTBEAT.wake();
    }

    #[cfg(debug_assertions)]
    eprintln!(
        "[bridge] {command:?} -> {} event(s): {events:?}",
        events.events.len()
    );

    Ok(events)
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

#[tauri::command]
fn fullscreen_state(window: tauri::WebviewWindow) -> fullscreen::FullscreenState {
    fullscreen::state(window.label())
}

#[tauri::command]
fn toggle_fullscreen(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<fullscreen::FullscreenState, String> {
    fullscreen::toggle(&window, &app)
}

/// A project the user actually named: `proton .` or `proton C:\src\thing`, the
/// way every editor takes it.
///
/// `None` means nothing was named, which is not the same as "the current
/// directory" — see [`working_folder`].
fn requested_folder() -> Option<std::path::PathBuf> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let s = args[i].to_string_lossy();
        if s == "--ssh" || s == "--remote-server" {
            i += 2;
            continue;
        }
        if s.starts_with('-') {
            i += 1;
            continue;
        }
        let path = std::path::PathBuf::from(&args[i]);
        return path.is_dir().then_some(path);
    }
    None
}

fn parse_ssh_target(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    if let Some((host, folder)) = trimmed.split_once(':') {
        let f = folder.trim();
        (
            host.trim().to_string(),
            (!f.is_empty()).then(|| f.to_string()),
        )
    } else {
        (trimmed.to_string(), None)
    }
}

fn startup_ssh_target() -> Option<(String, Option<String>)> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--ssh" {
            if let Some(target) = args.next() {
                return Some(parse_ssh_target(&target));
            }
        }
    }
    None
}

fn remote_server_arg() -> Option<Option<String>> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--remote-server" {
            let folder = args.next().filter(|a| !a.starts_with('-'));
            return Some(folder);
        }
    }
    None
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
    let session = windows
        .of(window.label())
        .ok_or_else(|| format!("{path}: window is closing"))?;
    let bytes = session.read_document_bytes(&path)?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command(async)]
fn save_document_as(app: tauri::AppHandle, path: String, window: tauri::WebviewWindow, windows: State<'_, Windows>) -> Result<Option<String>, String> {
    let session = windows
        .of(window.label())
        .ok_or_else(|| format!("{path}: window is closing"))?;
    let name = session.document_name(&path)?;
    let bytes = session.read_document_bytes(&path)?;
    // The panel is AppKit's, so it is shown on the main thread; this waits.
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(save_panel::choose(&name));
    })
    .map_err(|err| err.to_string())?;
    let Some(to) = rx.recv().map_err(|err| err.to_string())? else { return Ok(None) };
    std::fs::write(&to, bytes).map_err(|err| format!("{}: {err}", to.display()))?;
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

/// One poll of every local window's core, or `None` once the app is going away.
/// Remote SSH windows push events asynchronously on their own reader thread.
fn poll_once(app: &tauri::AppHandle) -> Option<Vec<(String, EventBatch)>> {
    let windows = app.try_state::<Windows>()?;
    Some(
        windows
            .all()
            .into_iter()
            .filter_map(|(label, session)| {
                let events = session.poll_local()?;
                Some((label, events))
            })
            .collect(),
    )
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
        let Ok(batch) = session.apply(Command::AskToQuit) else {
            continue;
        };
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
    let Ok(batch) = session.apply(Command::AskToClose) else {
        return false;
    };
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
            let failed = session.save_unsaved()?;
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
    session.shutdown(true, last);
}

#[tauri::command]
fn remote_status(window: tauri::WebviewWindow, windows: State<'_, Windows>) -> RemoteStatus {
    windows
        .of(window.label())
        .map(|s| s.remote_status())
        .unwrap_or_default()
}

#[tauri::command(async)]
fn remote_hosts() -> RemoteCatalog {
    RemoteCatalog {
        hosts: remote_client::discover_ssh_hosts(),
        recents: remote_client::load_recents(),
    }
}

#[tauri::command(async)]
fn remote_browse(host: String, path: Option<String>) -> Result<Vec<RemoteDirEntry>, String> {
    remote_client::browse_remote(&host, path.as_deref())
}

fn connect_client_for_label(
    app: &tauri::AppHandle,
    label: &str,
    host: &str,
    folder: Option<&str>,
) -> Result<Arc<RemoteClient>, String> {
    let app_push = app.clone();
    let label_push = label.to_string();
    let app_status = app.clone();
    let label_status = label.to_string();
    RemoteClient::connect_ssh(
        host,
        folder,
        move |batch| {
            let _ = app_push.emit(&channel(&label_push), batch);
        },
        move |status| {
            let _ = app_status.emit(&remote_status_channel(&label_status), status);
        },
    )
}

#[tauri::command(async)]
fn remote_connect(
    host: String,
    folder: Option<String>,
    new_window: bool,
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    windows: State<'_, Windows>,
) -> Result<RemoteStatus, String> {
    if new_window {
        let label = format!("window-{}", windows.opened.fetch_add(1, Ordering::SeqCst) + 2);
        let client = connect_client_for_label(&app, &label, &host, folder.as_deref())?;
        let status = client.status();
        windows.add(&label, WindowSession::Remote(client));
        if let Err(err) = build_and_open_window(&app, &label) {
            if let Some(removed) = windows.remove(&label) {
                removed.shutdown(false, false);
            }
            return Err(err);
        }
        return Ok(status);
    }

    let label = window.label().to_string();
    let client = connect_client_for_label(&app, &label, &host, folder.as_deref())?;
    let status = client.status();
    if let Some(prev) = windows.replace(&label, WindowSession::Remote(client)) {
        prev.shutdown(true, false);
    }
    let _ = app.emit(&remote_status_channel(&label), status.clone());
    Ok(status)
}

#[tauri::command(async)]
fn remote_disconnect(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    windows: State<'_, Windows>,
) -> Result<RemoteStatus, String> {
    let label = window.label().to_string();
    let local = WindowSession::new_local(Session::another_window());
    if let Some(prev) = windows.replace(&label, local) {
        prev.shutdown(false, false);
    }
    let status = RemoteStatus::default();
    let _ = app.emit(&remote_status_channel(&label), status.clone());
    Ok(status)
}

#[tauri::command(async)]
fn remote_forward_port(
    local_port: u16,
    remote_port: Option<u16>,
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    windows: State<'_, Windows>,
) -> Result<RemoteStatus, String> {
    let label = window.label().to_string();
    let session = windows
        .of(&label)
        .ok_or_else(|| "window is closing".to_string())?;
    let status = session.forward_port(local_port, remote_port.unwrap_or(local_port))?;
    let _ = app.emit(&remote_status_channel(&label), status.clone());
    Ok(status)
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
        tauri::WindowEvent::Focused(focused) => fullscreen::on_focus(this.label(), *focused),
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
            fullscreen::on_geometry_change(&this, this.app_handle());
        }
        tauri::WindowEvent::Destroyed => {
            fullscreen::on_destroyed(this.app_handle(), this.label());
            let_go(this.app_handle(), this.label());
        }
        _ => {}
    });
}

/// The menu's New Window, Connect to Host (SSH), and Full Screen toggle.
const NEW_WINDOW: &str = "proton-new-window";
const CONNECT_SSH: &str = "proton-connect-ssh";
const FULLSCREEN: &str = "proton-fullscreen";

fn build_and_open_window(app: &tauri::AppHandle, label: &str) -> Result<(), String> {
    let front = app
        .webview_windows()
        .into_values()
        .find(|w| w.is_focused().unwrap_or(false))
        .or_else(|| app.get_webview_window("main"));
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App("index.html".into()))
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
    let window = builder.build().map_err(|e| e.to_string())?;
    dress(&window, app);
    watch(&window);
    let _ = window.set_focus();
    Ok(())
}

/// Another window, with a session of its own on the home folder: in front of
/// the one in front, as large, a little down and to the right.
fn open_window(app: &tauri::AppHandle) -> Result<(), String> {
    let windows = app.try_state::<Windows>().ok_or("Proton is closing")?;
    let label = format!("window-{}", windows.opened.fetch_add(1, Ordering::SeqCst) + 2);
    // Before the window, so its page's first command finds it.
    windows.add(&label, WindowSession::new_local(Session::another_window()));
    if let Err(err) = build_and_open_window(app, &label) {
        if let Some(session) = windows.remove(&label) {
            session.shutdown(false, false);
        }
        return Err(err);
    }
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
        // File's first items: ⌘⇧N New Window, ⌘⇧O Connect to Host (SSH)...
        if submenu.text().is_ok_and(|text| text == "File") {
            let new = MenuItem::with_id(app, NEW_WINDOW, "New Window", true, Some("CmdOrCtrl+Shift+N")).ok()?;
            let ssh = MenuItem::with_id(app, CONNECT_SSH, "Connect to Host (SSH)…", true, Some("CmdOrCtrl+Shift+O")).ok()?;
            submenu.insert(&new, 0).ok()?;
            submenu.insert(&ssh, 1).ok()?;
        }
        for (at, entry) in submenu.items().ok()?.into_iter().enumerate() {
            let MenuItemKind::Predefined(predefined) = &entry else { continue };
            let Ok(text) = predefined.text() else { continue };
            if text.starts_with("Quit") {
                let quit = MenuItem::with_id(app, QUIT, "Quit Proton", true, Some("CmdOrCtrl+Q")).ok()?;
                submenu.remove(predefined).ok()?;
                submenu.insert(&quit, at).ok()?;
                quit_given = true;
            } else if text.contains("Full Screen") {
                let fs = MenuItem::with_id(
                    app,
                    FULLSCREEN,
                    "Toggle Full Screen",
                    true,
                    Some("Ctrl+Cmd+F"),
                )
                .ok()?;
                submenu.remove(predefined).ok()?;
                submenu.insert(&fs, at).ok()?;
            }
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
    if let Some(folder) = remote_server_arg() {
        std::process::exit(remote_server::run(folder));
    }

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
    let startup_ssh = startup_ssh_target();
    let windows = Windows::default();
    if startup_ssh.is_none() {
        windows.add(
            "main",
            WindowSession::new_local(Session::restore(requested_folder(), working_folder())),
        );
    }
    tauri::Builder::default()
        .manage(windows)
        .invoke_handler(tauri::generate_handler![
            dispatch,
            backdrop,
            set_backdrop,
            set_opaque,
            read_document,
            save_document_as,
            paste_text,
            copy_text,
            quit,
            remote_status,
            remote_hosts,
            remote_browse,
            remote_connect,
            remote_disconnect,
            remote_forward_port,
            fullscreen_state,
            toggle_fullscreen
        ])
        .setup(move |app| {
            let Some(window) = app.get_webview_window("main") else {
                // No window means the config is wrong, not that we should limp
                // on with a half-configured app.
                return Err("no main window in tauri.conf.json".into());
            };

            if let Some((host, folder)) = startup_ssh.clone() {
                let windows = app.state::<Windows>();
                match connect_client_for_label(app.handle(), "main", &host, folder.as_deref()) {
                    Ok(client) => {
                        windows.add("main", WindowSession::Remote(client));
                    }
                    Err(err) => {
                        eprintln!("proton: failed --ssh {host}: {err}");
                        windows.add(
                            "main",
                            WindowSession::new_local(Session::restore(None, working_folder())),
                        );
                    }
                }
            }

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
                if event.id() == CONNECT_SSH {
                    let target = app
                        .webview_windows()
                        .into_values()
                        .find(|w| w.is_focused().unwrap_or(false))
                        .map(|w| w.label().to_string())
                        .unwrap_or_else(|| "main".to_string());
                    let _ = app.emit(&open_remote_channel(&target), ());
                }
                if event.id() == FULLSCREEN {
                    let win = app
                        .webview_windows()
                        .into_values()
                        .find(|w| w.is_focused().unwrap_or(false))
                        .or_else(|| app.get_webview_window("main"));
                    if let Some(win) = win {
                        let _ = fullscreen::toggle(&win, app);
                    }
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
                        // A notebook's kernel leads a process group of
                        // its own, and the app exits
                        // without dropping the session: they would outlive
                        // it if not stopped here.
                        session.shutdown(true, true);
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
        let at = |dir: &str| WindowSession::new_local(Session::new(std::env::temp_dir().join(dir)));
        windows.add("main", at("proton-shell-main"));
        windows.add("window-2", at("proton-shell-second"));
        let main = windows.of("main").unwrap();
        let second = windows.of("window-2").unwrap();
        assert!(!Arc::ptr_eq(&main, &second), "two windows share a core");
        assert!(!second.is_remote());
        assert!(windows.of("window-3").is_none());
        assert!(windows.remove("window-2").is_some());
        assert!(windows.of("window-2").is_none() && windows.remove("window-2").is_none());
        assert_eq!(windows.all().iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["main"]);
        // Pushes go to the window they are for, by name.
        assert_eq!(channel("window-2"), "proton://events/window-2");
    }

    #[test]
    fn parses_cli_ssh_target_host_and_optional_folder() {
        assert_eq!(parse_ssh_target("mini"), ("mini".into(), None));
        assert_eq!(
            parse_ssh_target("mini:~/Github/Applications/Proton"),
            ("mini".into(), Some("~/Github/Applications/Proton".into()))
        );
        assert_eq!(parse_ssh_target("mini:"), ("mini".into(), None));
    }

    /// A window the capability does not name may not even listen: every
    /// window's label, the first's and the ones New Window makes, is in it.
    #[test]
    fn every_window_may_use_what_the_first_may() {
        let text = include_str!("../capabilities/default.json");
        let caps: serde_json::Value = serde_json::from_str(text).unwrap();
        let windows: Vec<&str> = caps["windows"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
        assert!(windows.contains(&"main"), "{windows:?}");
        let made = format!("window-{}", 2);
        let covered = windows.iter().any(|w| *w == made || w.strip_suffix('*').is_some_and(|prefix| made.starts_with(prefix)));
        assert!(covered, "a new window's label {made} is not in the capability's windows {windows:?}");
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
