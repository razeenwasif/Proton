//! The pen: pressure from a drawing tablet or Sidecar's Apple Pencil, read
//! from AppKit's events and told to the page.
//!
//! WebKit on the Mac gives a page no pen. A tablet's pen reaches it as a
//! mouse — `pointerType` "mouse", pressure nothing — and with no coalesced
//! events: a probe of this WebKit (2026-10-14) sent tablet-point mouse events
//! with pressure and tilt to a WKWebView, and the page saw neither. AppKit
//! has it all: a mouse event from a tablet has the subtype "tablet point" and
//! its pressure. So a local event monitor — Proton's own events, before the
//! window has them — reads each, and calls the page's `__protonPen` through
//! the WKWebView before the event goes on to it. Both reach the page over the
//! same connection, in that order, so the pressure is there by the time the
//! pointer event it belongs to is.
//!
//! What the page is told, as `__protonPen(what, pressure)`:
//!
//! - `"d"` and a pressure from 0 to 1: the pen is pressed, down or moving;
//! - `"u"`: it is lifted;
//! - `"n"` / `"e"`: its point, or its eraser end, came near the tablet;
//! - `"a"`: it went away.
//!
//! Only a tablet's events say anything; a mouse's and a trackpad's pass by.

/// What one event says of the pen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pen {
    /// Pressed, this hard, 0 to 1.
    Pressing(f32),
    Lifted,
    /// Near the tablet: its point, or its eraser end.
    Near { eraser: bool },
    Away,
}

/// AppKit's event types and subtypes this reads.
mod appkit {
    pub const LEFT_MOUSE_DOWN: u64 = 1;
    pub const LEFT_MOUSE_UP: u64 = 2;
    pub const LEFT_MOUSE_DRAGGED: u64 = 6;
    pub const TABLET_POINT: u64 = 23;
    pub const TABLET_PROXIMITY: u64 = 24;
    /// `NSEventSubtypeTabletPoint`, `NSEventSubtypeTabletProximity`.
    pub const SUBTYPE_POINT: i16 = 1;
    pub const SUBTYPE_PROXIMITY: i16 = 2;
    /// `NSPointingDeviceTypeEraser`.
    pub const ERASER: u64 = 3;
    /// The events watched: the left button's, and a tablet's own.
    pub const MASK: u64 = 1 << LEFT_MOUSE_DOWN | 1 << LEFT_MOUSE_UP | 1 << LEFT_MOUSE_DRAGGED | 1 << TABLET_POINT | 1 << TABLET_PROXIMITY;
}

/// What an event of `kind` and `subtype` says of the pen. Its pressure and
/// its proximity are asked only of the events that have them — AppKit raises
/// when asked of others.
pub fn read(kind: u64, subtype: i16, pressure: impl FnOnce() -> f32, proximity: impl FnOnce() -> (bool, u64)) -> Option<Pen> {
    use appkit::*;
    let mouse = matches!(kind, LEFT_MOUSE_DOWN | LEFT_MOUSE_UP | LEFT_MOUSE_DRAGGED);
    if kind == TABLET_PROXIMITY || (mouse && subtype == SUBTYPE_PROXIMITY) {
        let (entering, device) = proximity();
        return Some(if entering { Pen::Near { eraser: device == ERASER } } else { Pen::Away });
    }
    if kind == LEFT_MOUSE_UP && subtype == SUBTYPE_POINT {
        return Some(Pen::Lifted);
    }
    if (mouse && subtype == SUBTYPE_POINT) || kind == TABLET_POINT {
        let p = pressure();
        return Some(if p > 0.0 || kind != TABLET_POINT { Pen::Pressing(p.clamp(0.0, 1.0)) } else { Pen::Lifted });
    }
    None
}

/// What the page is told.
pub fn script(pen: Pen) -> String {
    let call = |what: &str, p: f32| format!("window.__protonPen&&window.__protonPen(\"{what}\",{p:.4})");
    match pen {
        Pen::Pressing(p) => call("d", p),
        Pen::Lifted => call("u", 0.0),
        Pen::Near { eraser } => call(if eraser { "e" } else { "n" }, 0.0),
        Pen::Away => call("a", 0.0),
    }
}

/// Watch the app's events for a pen, and tell `window`'s page of it.
#[cfg(target_os = "macos")]
pub fn watch(window: &tauri::WebviewWindow) {
    let done = window.with_webview(|webview| {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;

        // SAFETY: `inner` is this window's live WKWebView; retained here, it
        // lives as long as the monitor, which is as long as the app.
        let Some(view) = (unsafe { Retained::retain(webview.inner() as *mut AnyObject) }) else { return };
        let monitor = mac::monitor(move |event| {
            // SAFETY: AppKit hands the monitor a live event of a type in the
            // mask.
            if let Some(pen) = unsafe { mac::pen_of(event) } {
                mac::tell(&view, pen);
            }
        });
        // Kept for the app's life.
        std::mem::forget(monitor);
    });
    if let Err(err) = done {
        eprintln!("proton: could not watch for a pen: {err}");
    }
}

/// AppKit's side, apart so a test can make events and read them.
#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::CString;
    use std::ptr::NonNull;

    use block2::{Block, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};

    use super::{appkit, read, script, Pen};

    /// What an event says of the pen.
    ///
    /// # Safety
    /// `e` is a live NSEvent of a type in [`appkit::MASK`]: `read` asks for
    /// pressure and proximity only of the events that have them.
    pub unsafe fn pen_of(e: *mut AnyObject) -> Option<Pen> {
        let kind: u64 = msg_send![e, type];
        let subtype: i16 = msg_send![e, subtype];
        read(
            kind,
            subtype,
            || msg_send![e, pressure],
            || {
                let entering: Bool = msg_send![e, isEnteringProximity];
                let device: u64 = msg_send![e, pointingDeviceType];
                (entering.as_bool(), device)
            },
        )
    }

    /// Tell the page in `view`, a WKWebView.
    pub fn tell(view: &AnyObject, pen: Pen) {
        let Ok(text) = CString::new(script(pen)) else { return };
        // SAFETY: a C string made just now; the NSString is autoreleased into
        // the event loop's pool, and WebKit copies what it runs.
        unsafe {
            let js: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()];
            let none: Option<&Block<dyn Fn(*mut AnyObject, *mut AnyObject)>> = None;
            let _: () = msg_send![view, evaluateJavaScript: js, completionHandler: none];
        }
    }

    /// A local monitor of the events in [`appkit::MASK`], each handed to
    /// `each` and then let go on to the window.
    pub fn monitor(each: impl Fn(*mut AnyObject) + 'static) -> Option<Retained<AnyObject>> {
        let handler = RcBlock::new(move |event: NonNull<AnyObject>| -> *mut AnyObject {
            each(event.as_ptr());
            event.as_ptr()
        });
        // SAFETY: the mask is AppKit's; the handler is a block of the type the
        // call takes, which AppKit copies.
        unsafe {
            let monitor: *mut AnyObject = msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: appkit::MASK, handler: &*handler];
            Retained::retain(monitor)
        }
    }

    #[cfg(test)]
    mod tests {
        use std::ffi::c_void;

        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};

        use super::super::Pen;

        #[repr(C)]
        struct CGPoint {
            x: f64,
            y: f64,
        }

        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventCreateMouseEvent(source: *const c_void, kind: u32, at: CGPoint, button: u32) -> *mut c_void;
            fn CGEventSetIntegerValueField(event: *mut c_void, field: u32, value: i64);
            fn CGEventSetDoubleValueField(event: *mut c_void, field: u32, value: f64);
        }
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFRelease(cf: *const c_void);
        }

        /// A left button event as AppKit has it, from a tablet's pen at
        /// `pressure` or, with no pressure, from a mouse — made, never posted.
        fn event(kind: u32, pressure: Option<f64>) -> *mut AnyObject {
            // SAFETY: CoreGraphics' own calls on an event it made; AppKit's
            // copy is autoreleased, and the test reads it at once.
            unsafe {
                let cg = CGEventCreateMouseEvent(std::ptr::null(), kind, CGPoint { x: 10.0, y: 10.0 }, 0);
                if let Some(p) = pressure {
                    // kCGMouseEventSubtype: tablet point; kCGMouseEventPressure.
                    CGEventSetIntegerValueField(cg, 7, 1);
                    CGEventSetDoubleValueField(cg, 2, p);
                }
                let e: *mut AnyObject = msg_send![class!(NSEvent), eventWithCGEvent: cg];
                CFRelease(cg);
                e
            }
        }

        #[test]
        fn appkits_own_events_are_read_as_the_pen_or_not() {
            // SAFETY: events just made, of types in the mask.
            unsafe {
                assert_eq!(super::pen_of(event(1, None)), None, "a mouse is not a pen");
                match super::pen_of(event(6, Some(0.5))) {
                    Some(Pen::Pressing(p)) => assert!((p - 0.5).abs() < 0.01, "{p}"),
                    other => panic!("a pen dragged read as {other:?}"),
                }
                assert_eq!(super::pen_of(event(2, Some(0.0))), Some(Pen::Lifted));
            }
            // And the monitor is made — its block of the type AppKit takes —
            // and taken down.
            let monitor = super::monitor(|_| {}).expect("a monitor");
            // SAFETY: the monitor AppKit just gave.
            unsafe {
                let _: () = msg_send![class!(NSEvent), removeMonitor: &*monitor];
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn watch(_window: &tauri::WebviewWindow) {}

#[cfg(test)]
mod tests {
    use super::appkit::*;
    use super::*;

    const NEVER: fn() -> f32 = || panic!("asked for pressure");
    const FAR: fn() -> (bool, u64) = || panic!("asked for proximity");

    #[test]
    fn a_tablets_events_say_how_hard_the_pen_presses() {
        assert_eq!(read(LEFT_MOUSE_DOWN, SUBTYPE_POINT, || 0.25, FAR), Some(Pen::Pressing(0.25)));
        assert_eq!(read(LEFT_MOUSE_DRAGGED, SUBTYPE_POINT, || 0.9, FAR), Some(Pen::Pressing(0.9)));
        assert_eq!(read(LEFT_MOUSE_UP, SUBTYPE_POINT, NEVER, FAR), Some(Pen::Lifted));
        assert_eq!(read(TABLET_POINT, 0, || 0.5, FAR), Some(Pen::Pressing(0.5)));
        assert_eq!(read(TABLET_POINT, 0, || 0.0, FAR), Some(Pen::Lifted));
        assert_eq!(read(TABLET_PROXIMITY, 0, NEVER, || (true, ERASER)), Some(Pen::Near { eraser: true }));
        assert_eq!(read(LEFT_MOUSE_DOWN, SUBTYPE_PROXIMITY, NEVER, || (true, 1)), Some(Pen::Near { eraser: false }));
        assert_eq!(read(TABLET_PROXIMITY, 0, NEVER, || (false, 1)), Some(Pen::Away));
    }

    #[test]
    fn a_mouse_and_a_trackpad_say_nothing() {
        // Subtype 0 is a mouse's; 3 a trackpad's touches.
        for subtype in [0, 3] {
            for kind in [LEFT_MOUSE_DOWN, LEFT_MOUSE_DRAGGED, LEFT_MOUSE_UP] {
                assert_eq!(read(kind, subtype, NEVER, FAR), None);
            }
        }
    }

    #[test]
    fn the_page_is_told_in_a_line_it_runs_only_if_it_listens() {
        assert_eq!(script(Pen::Pressing(0.5)), r#"window.__protonPen&&window.__protonPen("d",0.5000)"#);
        assert_eq!(script(Pen::Near { eraser: true }), r#"window.__protonPen&&window.__protonPen("e",0.0000)"#);
        assert_eq!(MASK, 0b1_1000_0000_0000_0000_0100_0110);
    }
}
