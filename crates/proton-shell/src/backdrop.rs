//! The window backdrop.
//!
//! `backdrop-filter` inside the webview can only ever blur *page* content — it
//! cannot reach the desktop. The only thing that can put the wallpaper behind
//! Proton is the compositor, so this module asks the OS for a material and
//! tells the front end which one it got.
//!
//! Windows 11 offers two through `DWMWA_SYSTEMBACKDROP_TYPE`:
//!
//! - **Mica** samples the wallpaper only — a static, heavily blurred sample
//!   that re-tints as the window moves across the desktop. Nearly free.
//! - **Acrylic** samples everything behind the window, live, including other
//!   windows. Real GPU and battery cost.
//!
//! Mica is the default because Proton is a window you leave open all day, and
//! because "take on the colours of the wallpaper" is precisely what it is for.

use std::fmt;

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    #[test]
    fn a_theme_colour_is_read_as_three_channels() {
        assert_eq!(super::rgb("#ff0080"), Some((1.0, 0.0, 128.0 / 255.0)));
        assert_eq!(super::rgb(" #000000 "), Some((0.0, 0.0, 0.0)));
        assert_eq!(super::rgb("rgb(1, 2, 3)"), None);
        assert_eq!(super::rgb("#fff"), None);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    /// Wallpaper-tinted. Windows 11.
    #[default]
    Mica,
    /// Wallpaper-tinted, stronger. Windows 11.
    MicaAlt,
    /// Live blur of whatever is behind the window. Windows 10 1809+.
    Acrylic,
    /// macOS vibrancy.
    Vibrancy,
    /// No compositor material — the page paints its own ground.
    Solid,
}

impl fmt::Display for Backdrop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Backdrop::Mica => "mica",
            Backdrop::MicaAlt => "mica-alt",
            Backdrop::Acrylic => "acrylic",
            Backdrop::Vibrancy => "vibrancy",
            Backdrop::Solid => "solid",
        })
    }
}

/// Make the window opaque in `color`, or see-through again with `material`
/// behind it.
///
/// Codex paints every pixel of its own, so a see-through window under it only
/// buys work: the compositor blurs the desktop behind the window on every
/// frame it draws, for a material nobody can see through Codex's ground, and
/// blends the window with whatever is behind it. An opaque window is spared
/// both. The modes drawn as glass get their material back on the way out.
pub fn set_opaque(window: &tauri::WebviewWindow, color: Option<(f64, f64, f64)>, material: Backdrop) {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let Ok(handle) = window.ns_window() else {
            return;
        };
        let ns = handle as *mut AnyObject;
        match color {
            Some((red, green, blue)) => {
                let _ = window_vibrancy::clear_vibrancy(window);
                // SAFETY: `ns` is this window's live NSWindow, and the call is
                // on the main thread, where Tauri runs non-async commands.
                unsafe {
                    let ground: *mut AnyObject = msg_send![
                        class!(NSColor),
                        colorWithSRGBRed: red,
                        green: green,
                        blue: blue,
                        alpha: 1.0f64
                    ];
                    let _: () = msg_send![ns, setBackgroundColor: ground];
                    let _: () = msg_send![ns, setOpaque: true];
                }
            }
            None => {
                // SAFETY: as above.
                unsafe {
                    let clear: *mut AnyObject = msg_send![class!(NSColor), clearColor];
                    let _: () = msg_send![ns, setBackgroundColor: clear];
                    let _: () = msg_send![ns, setOpaque: false];
                }
                apply(window, material);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, color, material);
}

/// `#rrggbb` as three channels from 0 to 1.
pub fn rgb(hex: &str) -> Option<(f64, f64, f64)> {
    let hex = hex.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|v| f64::from(v) / 255.0);
    Some((channel(0)?, channel(2)?, channel(4)?))
}

/// Ask the compositor for `wanted`, and report what was actually applied.
///
/// Every failure path lands on [`Backdrop::Solid`], which the front end renders
/// by painting its own ground. That is not a broken state: Windows turns Mica
/// off when the user disables transparency effects or enters battery saver, and
/// it has to look deliberate when it does.
pub fn apply(window: &tauri::WebviewWindow, wanted: Backdrop) -> Backdrop {
    #[cfg(target_os = "windows")]
    let result = {
        // DWM holds whichever material was set last, so switching has to reset
        // the attribute first — otherwise asking for acrylic over mica keeps
        // mica and looks like the setting did nothing.
        let _ = window_vibrancy::clear_mica(window);
        match wanted {
            Backdrop::Mica => window_vibrancy::apply_mica(window, Some(true)),
            Backdrop::MicaAlt => window_vibrancy::apply_tabbed(window, Some(true)),
            Backdrop::Acrylic => window_vibrancy::apply_acrylic(window, None),
            Backdrop::Vibrancy | Backdrop::Solid => return Backdrop::Solid,
        }
    };

    #[cfg(target_os = "macos")]
    let result = match wanted {
        Backdrop::Solid => {
            let _ = window_vibrancy::clear_vibrancy(window);
            return Backdrop::Solid;
        }
        _ => window_vibrancy::apply_vibrancy(
            window,
            window_vibrancy::NSVisualEffectMaterial::HudWindow,
            None,
            Some(13.0),
        ),
    };

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let result: Result<(), window_vibrancy::Error> = {
        // No standard compositor material on Linux — it is per-compositor and
        // not worth guessing at. The painted ground is the answer there.
        let _ = (window, wanted);
        Err(window_vibrancy::Error::UnsupportedPlatform(
            "no system backdrop on this platform",
        ))
    };

    match result {
        Ok(()) => {
            #[cfg(target_os = "macos")]
            let applied = Backdrop::Vibrancy;
            #[cfg(not(target_os = "macos"))]
            let applied = wanted;
            applied
        }
        Err(err) => {
            eprintln!("proton: no {wanted} backdrop ({err}); painting our own ground");
            Backdrop::Solid
        }
    }
}
