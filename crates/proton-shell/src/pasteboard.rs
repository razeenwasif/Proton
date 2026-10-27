//! The system's clipboard, for the editor's ⌘V and what it yanks: the
//! editor's text is drawn, not an editable element, and WebKit gives a page
//! the clipboard only when something editable has the focus — so a page
//! paste there arrives late or not at all, and the shell reads it instead.

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

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::CString;

    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    pub fn general() -> *mut AnyObject {
        // SAFETY: a class method returning the shared pasteboard.
        unsafe { msg_send![class!(NSPasteboard), generalPasteboard] }
    }

    fn string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).expect("no NUL in a type name");
        // SAFETY: a C string made just now; the NSString is autoreleased.
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
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

    }
}
