// The window has no OS decorations — the titlebar, traffic lights and mode rail
// are all drawn in the page. Hiding the console subsystem keeps a stray terminal
// from appearing behind it on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backdrop;

use std::sync::Mutex;

use backdrop::Backdrop;
use proton_core::{Command, Event, Workspace};
use tauri::{Manager, State};

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
fn dispatch(command: Command, workspace: State<'_, Mutex<Workspace>>) -> Vec<Event> {
    let events = match workspace.lock() {
        Ok(mut ws) => ws.apply(command.clone()),
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
        .manage(Mutex::new(Workspace::new()))
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

            let applied = backdrop::apply(&window, Backdrop::default());
            println!("[shell] backdrop: {applied}");
            app.manage(Surface::new(applied));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Proton failed to start");
}
