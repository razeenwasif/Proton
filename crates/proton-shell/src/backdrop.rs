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
        Backdrop::Solid => return Backdrop::Solid,
        _ => window_vibrancy::apply_vibrancy(
            window,
            window_vibrancy::NSVisualEffectMaterial::HudWindow,
            None,
            None,
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
