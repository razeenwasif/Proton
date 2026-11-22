//! Where the window was, so it opens there again.
//!
//! Geometry is the shell's business and nobody else's. `proton-core` names no
//! widget and has no idea the window has a size, so this does not go through
//! the command bus — it is read once before the window is shown and written
//! back as the window moves.
//!
//! Stored beside the session, in the same folder and for the same reason: it
//! is config, and the sort of thing somebody should be able to delete when it
//! has ended up somewhere unhelpful.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{LogicalSize, PhysicalPosition, WebviewWindow, WindowEvent};

/// Bumped when the shape below changes. A file from another version is
/// ignored rather than guessed at, exactly as the session's is.
const VERSION: u32 = 1;

/// How long the window has to hold still before its geometry is written.
///
/// Not a throttle — a debounce, and the difference is what makes maximizing
/// work. Resizing emits an event per frame, so a write per event would be the
/// only expensive thing about dragging a corner; but more importantly, the
/// `Resized` that arrives when a window is maximized arrives while
/// `is_maximized()` still answers *false*. Saving from inside that event
/// records the screen-filling size as if it were an ordinary one, and the
/// size to return to is lost. Waiting for the window to settle and then
/// asking is the only way to be told the truth.
const SETTLE: Duration = Duration::from_millis(400);

/// How often to check whether it has settled.
const TICK: Duration = Duration::from_millis(120);

/// The geometry as it is stored between runs.
///
/// Size is **logical** and position is **physical**, which looks inconsistent
/// and is deliberate. A logical size keeps the window the same apparent size
/// when it opens on a monitor with a different scale factor — a physical one
/// would come back visibly smaller on a denser screen. Monitor layout, on the
/// other hand, is expressed in physical coordinates, so a physical position
/// is what can be checked against the monitors that actually exist.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    version: u32,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    /// Maximized windows remember the size they will return to, not the size
    /// of the screen they are filling.
    maximized: bool,
}

/// What the last write said, so a maximized window can be saved without
/// losing the size underneath it.
static LAST: Mutex<Option<Stored>> = Mutex::new(None);
/// When the window last moved, if it has not been written since.
static MOVED_AT: Mutex<Option<Instant>> = Mutex::new(None);

fn file() -> Option<PathBuf> {
    Some(proton_core::folder::config()?.join("window.json"))
}

fn read() -> Option<Stored> {
    let text = std::fs::read_to_string(file()?).ok()?;
    let stored: Stored = serde_json::from_str(&text).ok()?;
    (stored.version == VERSION).then_some(stored)
}

fn write(stored: &Stored) {
    let Some(path) = file() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(stored) {
        // A geometry that cannot be written is not worth interrupting anybody
        // over: the window is already open and the right size.
        let _ = std::fs::write(&path, text);
    }
}

/// Whether a window put here would be reachable.
///
/// The bug this exists for: close on a second monitor, unplug it, reopen. The
/// stored position is still perfectly valid and describes a place that is no
/// longer on any screen, so the window opens invisibly and the app looks like
/// it failed to start.
///
/// A little way *inside* the top-left corner rather than the corner itself,
/// because the corner can legitimately sit a pixel outside a monitor while
/// the window is fine, and because the part that has to be reachable is the
/// strip you can grab to drag it back.
fn reachable(window: &WebviewWindow, x: i32, y: i32) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };
    let screens: Vec<Screen> = monitors
        .iter()
        .map(|monitor| {
            let at = monitor.position();
            let size = monitor.size();
            Screen {
                x: at.x,
                y: at.y,
                width: size.width,
                height: size.height,
            }
        })
        .collect();
    on_a_screen(&screens, x, y)
}

/// A monitor, as a rectangle in physical coordinates.
#[derive(Debug, Clone, Copy)]
struct Screen {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// The decision itself, over plain rectangles.
///
/// Split out from the window so it can be tested: a second monitor cannot be
/// unplugged from inside a test, and this is the part that goes wrong when
/// one is.
fn on_a_screen(screens: &[Screen], x: i32, y: i32) -> bool {
    let (grab_x, grab_y) = (x + GRAB.0, y + GRAB.1);
    screens.iter().any(|screen| {
        let right = screen.x + screen.width as i32;
        let bottom = screen.y + screen.height as i32;
        grab_x >= screen.x && grab_x < right && grab_y >= screen.y && grab_y < bottom
    })
}

/// A point inside the strip you can grab to drag the window.
const GRAB: (i32, i32) = (48, 16);

/// Put the window back where it was. Called before it is shown.
pub fn restore(window: &WebviewWindow) {
    let Some(stored) = read() else { return };
    *LAST.lock().unwrap() = Some(stored.clone());

    // The minimums in `tauri.conf.json` are the floor. A stored size below
    // them came from a different build, and the window manager would clamp it
    // anyway — better to ignore it than to ask for something impossible.
    if stored.width >= 400 && stored.height >= 300 {
        let _ = window.set_size(LogicalSize::new(stored.width, stored.height));
    }
    if reachable(window, stored.x, stored.y) {
        let _ = window.set_position(PhysicalPosition::new(stored.x, stored.y));
    }
    if stored.maximized {
        let _ = window.maximize();
    }
}

/// What the window looks like now, or `None` if it is in no state to be asked.
fn snapshot(window: &WebviewWindow) -> Option<Stored> {
    // A minimized window reports a size that means nothing — saving it would
    // reopen the app as a sliver.
    if window.is_minimized().unwrap_or(false) {
        return None;
    }
    if crate::fullscreen::is_active(window.label()) || window.is_fullscreen().unwrap_or(false) {
        return None;
    }

    let previous = LAST.lock().unwrap().clone();
    if window.is_maximized().unwrap_or(false) {
        // Keep the size it will return to. Saving the screen-filling size
        // would mean unmaximizing did nothing next time.
        return Some(Stored {
            maximized: true,
            ..previous?
        });
    }

    let scale = window.scale_factor().ok()?;
    // `inner_size`, not `outer_size`, because `set_size` sets the *inner*
    // one. Saving the outer size and restoring it as the inner size adds the
    // window frame back on every launch — measured at 16x9 here, so the
    // window crept larger each time it was opened: 1240 to 1256 to 1272.
    // Position has no such asymmetry: `outer_position` and `set_position` are
    // both the outer one, which is why it restored exactly while size did not.
    let size = window.inner_size().ok()?;
    let at = window.outer_position().ok()?;
    Some(Stored {
        version: VERSION,
        width: (f64::from(size.width) / scale).round() as u32,
        height: (f64::from(size.height) / scale).round() as u32,
        x: at.x,
        y: at.y,
        maximized: false,
    })
}

/// Write what the window looks like now.
fn save(window: &WebviewWindow) {
    *MOVED_AT.lock().unwrap() = None;
    let Some(stored) = snapshot(window) else {
        return;
    };
    *LAST.lock().unwrap() = Some(stored.clone());
    write(&stored);
}

/// Start remembering where this window is.
pub fn remember(window: &WebviewWindow) {
    // On a first run there is no stored size, so the first thing that happens
    // has nothing to fall back on. Without this, somebody who opens Proton and
    // immediately maximizes it saves nothing at all: `snapshot` has no size to
    // keep underneath the maximized one, and gives up. Seed it with the
    // defaults the window was just built with.
    // Each lock is its own statement on purpose: `snapshot` takes the same
    // one, so holding a guard across the call would deadlock the app on
    // startup.
    let unseeded = LAST.lock().unwrap().is_none();
    if unseeded {
        let seeded = snapshot(window);
        *LAST.lock().unwrap() = seeded;
    }

    // Writes what the events below only note, once the window has held still
    // long enough to answer honestly about being maximized. Asleep until the
    // window moves: a window that is left where it is costs no wake-ups at all,
    // where this used to look eight times a second for as long as it was open.
    let ticking = window.clone();
    let ticker = std::thread::spawn(move || loop {
        let moved = *MOVED_AT.lock().unwrap();
        match moved {
            None => std::thread::park(),
            Some(at) if at.elapsed() >= SETTLE => save(&ticking),
            Some(_) => std::thread::sleep(TICK),
        }
    });
    let ticker = ticker.thread().clone();

    let watched = window.clone();
    window.on_window_event(move |event| match event {
        // Noted, not saved. These arrive once a frame while a corner is
        // dragged, and the one that arrives on maximizing arrives too early
        // to be believed — see `SETTLE`.
        WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
            *MOVED_AT.lock().unwrap() = Some(Instant::now());
            ticker.unpark();
        }
        // The last word. By the time a window is closing it has long since
        // settled, so this can ask directly.
        WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => save(&watched),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(x: i32, y: i32, width: u32, height: u32) -> Screen {
        Screen {
            x,
            y,
            width,
            height,
        }
    }

    /// A laptop with a second monitor to the left of it, which is where
    /// negative coordinates come from and why the bound is not `0`.
    fn two() -> Vec<Screen> {
        vec![screen(0, 0, 1920, 1080), screen(-2560, -200, 2560, 1440)]
    }

    #[test]
    fn a_window_on_either_monitor_is_reachable() {
        assert!(on_a_screen(&two(), 100, 100));
        assert!(on_a_screen(&two(), -2000, 0), "the left-hand monitor");
    }

    #[test]
    fn a_window_on_a_monitor_that_is_gone_is_not() {
        // Close on the second monitor, unplug it, reopen. The position is
        // still valid and describes nowhere, so the window would open
        // invisibly and the app would look like it failed to start.
        let only_the_laptop = vec![screen(0, 0, 1920, 1080)];
        assert!(!on_a_screen(&only_the_laptop, -2000, 0));
        assert!(
            !on_a_screen(&only_the_laptop, 4000, 300),
            "and off the right"
        );
        assert!(!on_a_screen(&only_the_laptop, 100, 2000), "and below");
    }

    #[test]
    fn the_point_checked_is_the_one_you_can_grab() {
        // A corner a little off the top-left of a screen is fine — the strip
        // you drag by is still on it. This is why the corner itself is not
        // what gets tested.
        let one = vec![screen(0, 0, 1920, 1080)];
        assert!(on_a_screen(&one, -GRAB.0 + 1, -GRAB.1 + 1));
        // Far enough out that the strip is gone too.
        assert!(!on_a_screen(&one, -GRAB.0 - 1, -GRAB.1 - 1));
    }

    #[test]
    fn a_stored_geometry_survives_a_round_trip() {
        // The file is the contract between two runs, so it has to read back
        // as what was written — and a file from another version must not.
        let stored = Stored {
            version: VERSION,
            width: 1440,
            height: 900,
            x: -120,
            y: 40,
            maximized: false,
        };
        let text = serde_json::to_string(&stored).unwrap();
        let read: Stored = serde_json::from_str(&text).unwrap();
        assert_eq!((read.width, read.height), (1440, 900));
        assert_eq!((read.x, read.y), (-120, 40));
        assert!(!read.maximized);

        let old = text.replace(&format!("\"version\":{VERSION}"), "\"version\":999");
        let parsed: Stored = serde_json::from_str(&old).unwrap();
        assert_ne!(
            parsed.version, VERSION,
            "and is refused by the version check"
        );
    }
}
