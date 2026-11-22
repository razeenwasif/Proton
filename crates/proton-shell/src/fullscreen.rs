//! Notch-aware borderless full screen (`simple_fullscreen`) for macOS.
//!
//! Standard macOS Spaces full screen pushes undecorated apps into a separate
//! Space under a solid black bar below the camera housing unless configured,
//! and reserves the entire top bar. Entering `simple_fullscreen` with
//! `NSPrefersDisplaySafeAreaCompatibilityMode = false` expands the `NSWindow`
//! across `NSScreen.frame` — wrapping seamlessly around the notch — while
//! exposing the notch bounds to the UI so center panes avoid the cutout.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

/// Full-screen layout and hardware notch geometry (in logical points from the
/// active screen's top-left corner) pushed to each window over
/// `proton://fullscreen/<label>`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullscreenState {
    pub active: bool,
    pub notch_height: f64,
    pub notch_width: f64,
    pub notch_left: f64,
}

pub fn channel(label: &str) -> String {
    format!("proton://fullscreen/{label}")
}

static ACTIVE: Mutex<Option<HashMap<String, FullscreenState>>> = Mutex::new(None);

fn lock_active() -> MutexGuard<'static, Option<HashMap<String, FullscreenState>>> {
    ACTIVE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_active(label: &str) -> bool {
    lock_active()
        .as_ref()
        .is_some_and(|map| map.get(label).is_some_and(|s| s.active))
}

pub fn state(label: &str) -> FullscreenState {
    lock_active()
        .as_ref()
        .and_then(|map| map.get(label).cloned())
        .unwrap_or_default()
}

/// Compute `(notch_height, notch_width, notch_left)` in logical points from
/// `NSScreen` metrics (`frame`, `safeAreaInsets.top`, `auxiliaryTopLeftArea`,
/// and `auxiliaryTopRightArea`).
pub fn compute_notch_geometry(
    screen_frame: (f64, f64, f64, f64),
    safe_top: f64,
    aux_left: Option<(f64, f64, f64, f64)>,
    aux_right: Option<(f64, f64, f64, f64)>,
) -> (f64, f64, f64) {
    let aux_h = aux_left
        .map_or(0.0, |r| r.3)
        .max(aux_right.map_or(0.0, |r| r.3));
    let notch_height = safe_top.max(aux_h).max(0.0);
    if notch_height <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    if let (Some(l), Some(r)) = (aux_left, aux_right) {
        if l.2 > 0.0 && l.3 > 0.0 && r.2 > 0.0 && r.3 > 0.0 {
            let left = ((l.0 + l.2) - screen_frame.0).max(0.0);
            let right = (r.0 - screen_frame.0).max(left);
            let width = (right - left).max(0.0);
            return (notch_height, width, left);
        }
    }
    (notch_height, 0.0, 0.0)
}

pub fn set(
    window: &tauri::WebviewWindow,
    app: &tauri::AppHandle,
    enable: bool,
) -> Result<FullscreenState, String> {
    if !enable && window.is_fullscreen().unwrap_or(false) {
        window.set_fullscreen(false).map_err(|e| e.to_string())?;
    }
    window
        .set_simple_fullscreen(enable)
        .map_err(|e| e.to_string())?;

    let next = if enable {
        let (notch_height, notch_width, notch_left) = query_screen_notch(window);
        FullscreenState {
            active: true,
            notch_height,
            notch_width,
            notch_left,
        }
    } else {
        FullscreenState::default()
    };

    {
        let mut guard = lock_active();
        let map = guard.get_or_insert_with(HashMap::new);
        if enable {
            map.insert(window.label().to_string(), next.clone());
        } else {
            map.remove(window.label());
        }
    }

    #[cfg(target_os = "macos")]
    {
        apply_window_chrome(window, enable, app);
        set_presentation_options(enable);
    }

    let _ = app.emit(&channel(window.label()), &next);
    Ok(next)
}

pub fn toggle(
    window: &tauri::WebviewWindow,
    app: &tauri::AppHandle,
) -> Result<FullscreenState, String> {
    if window.is_fullscreen().unwrap_or(false) {
        window.set_fullscreen(false).map_err(|e| e.to_string())?;
        {
            if let Some(map) = lock_active().as_mut() {
                map.remove(window.label());
            }
        }
        let next = FullscreenState::default();
        let _ = app.emit(&channel(window.label()), &next);
        return Ok(next);
    }
    set(window, app, !is_active(window.label()))
}

pub fn on_focus(label: &str, focused: bool) {
    #[cfg(target_os = "macos")]
    if focused {
        set_presentation_options(is_active(label));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (label, focused);
}

pub fn on_geometry_change(window: &tauri::WebviewWindow, app: &tauri::AppHandle) {
    let label = window.label();
    if !is_active(label) {
        return;
    }
    let (notch_height, notch_width, notch_left) = query_screen_notch(window);
    let next = FullscreenState {
        active: true,
        notch_height,
        notch_width,
        notch_left,
    };
    let changed = {
        let mut guard = lock_active();
        let map = guard.get_or_insert_with(HashMap::new);
        if map.get(label) == Some(&next) {
            false
        } else {
            map.insert(label.to_string(), next.clone());
            true
        }
    };
    if changed {
        let _ = app.emit(&channel(label), &next);
    }
}

pub fn on_destroyed(app: &tauri::AppHandle, label: &str) {
    if let Some(map) = lock_active().as_mut() {
        map.remove(label);
    }
    #[cfg(target_os = "macos")]
    {
        let still_active = app
            .webview_windows()
            .into_values()
            .filter(|w| w.label() != label && w.is_focused().unwrap_or(false))
            .any(|w| is_active(w.label()));
        set_presentation_options(still_active);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::encode::{Encode, Encoding};
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, sel};
    use tauri::Manager;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    unsafe impl Encode for CGPoint {
        const ENCODING: Encoding =
            Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    unsafe impl Encode for CGSize {
        const ENCODING: Encoding =
            Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    unsafe impl Encode for CGRect {
        const ENCODING: Encoding =
            Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
    }

    impl CGRect {
        fn tuple(self) -> (f64, f64, f64, f64) {
            (self.origin.x, self.origin.y, self.size.width, self.size.height)
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct NSEdgeInsets {
        top: f64,
        left: f64,
        bottom: f64,
        right: f64,
    }

    unsafe impl Encode for NSEdgeInsets {
        const ENCODING: Encoding = Encoding::Struct(
            "NSEdgeInsets",
            &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
        );
    }

    pub fn query_screen_notch(window: &tauri::WebviewWindow) -> (f64, f64, f64) {
        let Ok(handle) = window.ns_window() else {
            return (0.0, 0.0, 0.0);
        };
        let ns = handle as *mut AnyObject;
        // SAFETY: `ns` is this window's live `NSWindow` and AppKit calls run on
        // the main thread. `respondsToSelector:` guards macOS 12+ notch selectors.
        unsafe {
            let mut screen: *mut AnyObject = msg_send![ns, screen];
            if screen.is_null() {
                screen = msg_send![class!(NSScreen), mainScreen];
            }
            if screen.is_null() {
                return (0.0, 0.0, 0.0);
            }
            let frame: CGRect = msg_send![screen, frame];
            let has_safe: bool = msg_send![screen, respondsToSelector: sel!(safeAreaInsets)];
            let safe_top = if has_safe {
                let insets: NSEdgeInsets = msg_send![screen, safeAreaInsets];
                insets.top
            } else {
                0.0
            };
            let has_left: bool =
                msg_send![screen, respondsToSelector: sel!(auxiliaryTopLeftArea)];
            let aux_left = if has_left {
                let r: CGRect = msg_send![screen, auxiliaryTopLeftArea];
                Some(r.tuple())
            } else {
                None
            };
            let has_right: bool =
                msg_send![screen, respondsToSelector: sel!(auxiliaryTopRightArea)];
            let aux_right = if has_right {
                let r: CGRect = msg_send![screen, auxiliaryTopRightArea];
                Some(r.tuple())
            } else {
                None
            };
            super::compute_notch_geometry(frame.tuple(), safe_top, aux_left, aux_right)
        }
    }

    pub fn apply_window_chrome(
        window: &tauri::WebviewWindow,
        enable: bool,
        app: &tauri::AppHandle,
    ) {
        let Ok(handle) = window.ns_window() else {
            return;
        };
        let ns = handle as *mut AnyObject;
        // SAFETY: `ns` is this window's live `NSWindow` on the main thread.
        let opaque: bool = unsafe {
            let _: () = msg_send![ns, setHasShadow: !enable];
            msg_send![ns, isOpaque]
        };
        if !opaque {
            let material = app
                .try_state::<crate::Surface>()
                .and_then(|s| s.lock().ok().map(|g| *g))
                .unwrap_or_default();
            crate::backdrop::apply_with_radius(
                window,
                material,
                Some(if enable { 0.0 } else { 13.0 }),
            );
        }
    }

    pub fn set_presentation_options(auto_hide: bool) {
        const AUTO_HIDE_DOCK: usize = 1 << 0;
        const AUTO_HIDE_MENU_BAR: usize = 1 << 2;
        let opts: usize = if auto_hide {
            AUTO_HIDE_DOCK | AUTO_HIDE_MENU_BAR
        } else {
            0
        };
        // SAFETY: `[NSApplication sharedApplication]` is always valid on the
        // main thread once AppKit is running.
        unsafe {
            let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            if !ns_app.is_null() {
                let _: () = msg_send![ns_app, setPresentationOptions: opts];
            }
        }
    }
}

#[cfg(target_os = "macos")]
use mac::{apply_window_chrome, query_screen_notch, set_presentation_options};

#[cfg(not(target_os = "macos"))]
fn query_screen_notch(_window: &tauri::WebviewWindow) -> (f64, f64, f64) {
    (0.0, 0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_notch_geometry_for_14_inch_macbook_pro() {
        let (h, w, l) = compute_notch_geometry(
            (0.0, 0.0, 1512.0, 982.0),
            32.0,
            Some((0.0, 950.0, 663.5, 32.0)),
            Some((848.5, 950.0, 663.5, 32.0)),
        );
        assert_eq!((h, w, l), (32.0, 185.0, 663.5));
    }

    #[test]
    fn computes_notch_geometry_for_offset_display() {
        let (h, w, l) = compute_notch_geometry(
            (-1512.0, 0.0, 1512.0, 982.0),
            32.0,
            Some((-1512.0, 950.0, 663.5, 32.0)),
            Some((-663.5, 950.0, 663.5, 32.0)),
        );
        assert_eq!((h, w, l), (32.0, 185.0, 663.5));
    }

    #[test]
    fn computes_zero_notch_for_regular_display() {
        assert_eq!(
            compute_notch_geometry((0.0, 0.0, 2560.0, 1440.0), 0.0, None, None),
            (0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn fullscreen_state_serializes_camel_case() {
        let state = FullscreenState {
            active: true,
            notch_height: 32.0,
            notch_width: 185.0,
            notch_left: 663.5,
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["active"], true);
        assert_eq!(json["notchHeight"], 32.0);
        assert_eq!(json["notchWidth"], 185.0);
        assert_eq!(json["notchLeft"], 663.5);
    }
}
