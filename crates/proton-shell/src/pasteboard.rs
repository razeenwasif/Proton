//! Pictures on the system's clipboard, for Canvas mode — and its text, for
//! the editor's ⌘V, on the same terms: the editor's text is drawn, not an
//! editable element, so a page paste there arrives late or not at all.
//!
//! WebKit does not give a page what is on the clipboard unless something
//! editable has the focus, and a canvas is not that; so ⌘V asks here
//! instead. A copy in Canvas puts its pixels on the clipboard as a PNG too,
//! for other apps — and remembers the clipboard's change count after, so
//! that ⌘V with nothing copied since pastes Canvas's own copy (where it was,
//! every pixel as it is kept) rather than the PNG made of it. A picture
//! copied anywhere else is read as a PNG, or a TIFF made a PNG.

use std::sync::atomic::{AtomicIsize, Ordering};

/// The clipboard's change count after Canvas last wrote to it.
static OURS: AtomicIsize = AtomicIsize::new(-1);

/// What ⌘V should paste: a picture another app copied, as a PNG; none if
/// the clipboard holds Canvas's own copy, or no picture.
pub fn take() -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        let board = mac::general();
        // SAFETY: the general pasteboard, which AppKit keeps alive.
        unsafe {
            if mac::change_count(board) == OURS.load(Ordering::SeqCst) {
                return None;
            }
            mac::read_png(board)
        }
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// The text on the clipboard, for the editor's ⌘V: none if it holds none.
pub fn text() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: the general pasteboard, which AppKit keeps alive.
        unsafe { mac::read_text(mac::general()) }
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// Put text on the clipboard: what the editor yanked or deleted.
pub fn put_text(text: &str) {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: the general pasteboard, which AppKit keeps alive.
        unsafe { mac::write_text(mac::general(), text) };
    }
    #[cfg(not(target_os = "macos"))]
    let _ = text;
}

/// Put a PNG on the clipboard, as Canvas's copy.
pub fn put(png: &[u8]) {
    #[cfg(target_os = "macos")]
    {
        let board = mac::general();
        // SAFETY: as in `take`.
        let count = unsafe { mac::write_png(board, png) };
        OURS.store(count, Ordering::SeqCst);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = png;
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_void, CString};

    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    /// `NSBitmapImageFileTypePNG`.
    const PNG: u64 = 4;

    pub fn general() -> *mut AnyObject {
        // SAFETY: a class method returning the shared pasteboard.
        unsafe { msg_send![class!(NSPasteboard), generalPasteboard] }
    }

    fn string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).expect("no NUL in a type name");
        // SAFETY: a C string made just now; the NSString is autoreleased.
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }

    /// An NSData's bytes, copied.
    unsafe fn bytes(data: *mut AnyObject) -> Option<Vec<u8>> {
        if data.is_null() {
            return None;
        }
        let length: usize = msg_send![data, length];
        let at: *const c_void = msg_send![data, bytes];
        if at.is_null() || length == 0 {
            return None;
        }
        Some(std::slice::from_raw_parts(at as *const u8, length).to_vec())
    }

    /// # Safety
    /// `board` is a live NSPasteboard.
    pub unsafe fn change_count(board: *mut AnyObject) -> isize {
        msg_send![board, changeCount]
    }

    /// A picture on `board` as a PNG: its PNG, or its TIFF made one.
    ///
    /// # Safety
    /// `board` is a live NSPasteboard.
    pub unsafe fn read_png(board: *mut AnyObject) -> Option<Vec<u8>> {
        let png: *mut AnyObject = msg_send![board, dataForType: string("public.png")];
        if let Some(found) = bytes(png) {
            return Some(found);
        }
        let tiff: *mut AnyObject = msg_send![board, dataForType: string("public.tiff")];
        if tiff.is_null() {
            return None;
        }
        let image: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
        if image.is_null() {
            return None;
        }
        let none: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        let made: *mut AnyObject = msg_send![image, representationUsingType: PNG, properties: none];
        bytes(made)
    }

    /// The plain text on `board`, as UTF-8.
    ///
    /// # Safety
    /// `board` is a live NSPasteboard.
    pub unsafe fn read_text(board: *mut AnyObject) -> Option<String> {
        let text: *mut AnyObject = msg_send![board, stringForType: string("public.utf8-plain-text")];
        if text.is_null() {
            return None;
        }
        let utf8: *const std::ffi::c_char = msg_send![text, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }

    /// Put `text` on `board`, alone.
    ///
    /// # Safety
    /// `board` is a live NSPasteboard.
    pub unsafe fn write_text(board: *mut AnyObject, text: &str) {
        let Ok(c) = CString::new(text.replace('\0', "")) else { return };
        let ns: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
        if ns.is_null() {
            return;
        }
        let _: isize = msg_send![board, clearContents];
        let _: bool = msg_send![board, setString: ns, forType: string("public.utf8-plain-text")];
    }

    /// Put `png` on `board`, alone; the change count after.
    ///
    /// # Safety
    /// `board` is a live NSPasteboard.
    pub unsafe fn write_png(board: *mut AnyObject, png: &[u8]) -> isize {
        let _: isize = msg_send![board, clearContents];
        let data: *mut AnyObject = msg_send![class!(NSData), dataWithBytes: png.as_ptr() as *const c_void, length: png.len()];
        let _: bool = msg_send![board, setData: data, forType: string("public.png")];
        change_count(board)
    }

    #[cfg(test)]
    mod tests {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};

        /// A pasteboard of the test's own — never the person's clipboard.
        fn own() -> *mut AnyObject {
            // SAFETY: a class method making a fresh pasteboard.
            unsafe { msg_send![class!(NSPasteboard), pasteboardWithUniqueName] }
        }

        #[test]
        fn text_put_on_a_pasteboard_is_read_back_as_it_was() {
            let board = own();
            // SAFETY: the test's own pasteboard, alive until released below.
            unsafe {
                assert_eq!(super::read_text(board), None, "a new pasteboard holds text");
                super::write_text(board, "● a point — and 中文\nint x;");
                assert_eq!(super::read_text(board).as_deref(), Some("● a point — and 中文\nint x;"));
                super::write_text(board, "yanked\n");
                assert_eq!(super::read_text(board).as_deref(), Some("yanked\n"), "a second write did not replace the first");
                let _: () = msg_send![board, releaseGlobally];
            }
        }

        #[test]
        fn a_png_put_on_a_pasteboard_is_read_back_and_a_tiff_is_made_a_png() {
            // Opaque: a TIFF may keep a half-clear pixel's colour premultiplied.
            let rgba: Vec<u8> = (0..6 * 4 * 4).map(|i| if i % 4 == 3 { 255 } else { (i * 7 % 256) as u8 }).collect();
            let png = proton_raster::file::encode_png(6, 4, &rgba).unwrap();
            let board = own();
            // SAFETY: the test's own pasteboard, alive until released below.
            unsafe {
                let before = super::change_count(board);
                let after = super::write_png(board, &png);
                assert!(after > before);
                assert_eq!(super::read_png(board).unwrap(), png);
                // A TIFF, as many apps copy a picture: read as a PNG of the
                // same pixels.
                let data: *mut AnyObject = msg_send![class!(NSData), dataWithBytes: png.as_ptr() as *const std::ffi::c_void, length: png.len()];
                let image: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: data];
                let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
                let _: isize = msg_send![board, clearContents];
                let _: bool = msg_send![board, setData: tiff, forType: super::string("public.tiff")];
                let back = super::read_png(board).expect("the TIFF read as a PNG");
                let (w, h, pixels) = proton_raster::file::decode_png(&back).unwrap();
                assert_eq!((w, h), (6, 4));
                assert_eq!(&pixels[..4], &rgba[..4]);
                let _: () = msg_send![board, releaseGlobally];
            }
        }
    }
}
