// The window has no OS decorations — the titlebar, traffic lights and mode rail
// are all drawn in the page. Hiding the console subsystem keeps a stray terminal
// from appearing behind it on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backdrop;
mod window;

use std::sync::Mutex;
use std::time::Duration;

use backdrop::Backdrop;
use proton_core::{Command, Event, Session};
use tauri::{Emitter, Manager, State};

/// The channel language servers arrive on. Distinct from the `dispatch` reply
/// path because nothing asked for these: a diagnostic shows up because a
/// server finished thinking, not because the user pressed a key.
const PUSHED: &str = "proton://events";

/// How often to ask the core whether a server has said anything.
///
/// The core has no clock and no threads — that is what makes it testable — so
/// something has to ask on its behalf, and this is that something. Fast enough
/// that a completion list feels like it arrived with the keystroke, slow
/// enough to be free: an idle poll is a `try_recv` on a handful of channels
/// and returns no events, so it never redraws anything.
const BEAT: Duration = Duration::from_millis(50);

/// What the compositor actually gave us. The front end needs this to decide
/// whether to paint its own ground, so it reads it on boot and after any change.
type Surface = Mutex<Backdrop>;

/// The whole bridge. One command in, the events it produced out.
///
/// M0 answers synchronously because every command it handles is synchronous.
/// Streaming work — agent deltas, trace steps, LSP diagnostics — arrives in M3
/// and will push through `AppHandle::emit` instead; the `Event` type is already
/// the right shape for both, which is the point of keeping it transport-
/// agnostic.
#[tauri::command]
fn dispatch(command: Command, session: State<'_, Mutex<Session>>) -> Vec<Event> {
    let events = match session.lock() {
        Ok(mut it) => it.apply(command.clone()),
        // A poisoned lock means a previous command panicked. Recover rather
        // than propagating: the front end should keep working, and the panic
        // has already been reported on stderr.
        Err(poisoned) => poisoned.into_inner().apply(command.clone()),
    };

    // A dropped keystroke is the worst failure mode a modal editor has, and it
    // is invisible from the page. Tracing the seam in debug builds makes it
    // visible — and is how M0's exit criterion gets verified without a
    // screenshot.
    #[cfg(debug_assertions)]
    eprintln!(
        "[bridge] {command:?} -> {} event(s): {events:?}",
        events.len()
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
/// `None` under a Start-menu shortcut, where the working directory is the
/// install folder and indexing that would be nonsense.
fn working_folder() -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let beside_the_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .is_some_and(|dir| dir == cwd);
    (!beside_the_exe).then_some(cwd)
}

/// Give the core a heartbeat.
///
/// Everything a language server says arrives on a thread the core cannot see,
/// so the mailbox has to be drained by someone with a clock. This is the whole
/// of that arrangement: ask, and if anything came back, push it at the page.
/// Nothing is emitted when nothing happened, which is the common case by
/// several orders of magnitude.
fn spawn_poll(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("proton-poll".into())
        .spawn(move || loop {
            std::thread::sleep(BEAT);

            let Some(session) = app.try_state::<Mutex<Session>>() else {
                // The app is shutting down and the state is gone.
                return;
            };
            let events = match session.lock() {
                Ok(mut it) => it.apply(Command::Poll),
                Err(poisoned) => poisoned.into_inner().apply(Command::Poll),
            };

            if events.is_empty() {
                continue;
            }
            #[cfg(debug_assertions)]
            eprintln!("[poll] {} event(s): {events:?}", events.len());
            if app.emit(PUSHED, events).is_err() {
                return;
            }
        })
        .expect("the poll thread is not optional: without it no server is ever heard from");
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

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(Session::restore(
            requested_folder(),
            working_folder(),
        )))
        .invoke_handler(tauri::generate_handler![dispatch, backdrop, set_backdrop])
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
        .run(tauri::generate_context!())
        .expect("Proton failed to start");
}
