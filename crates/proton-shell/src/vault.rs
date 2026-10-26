//! Diffuse's key, and the person's say-so before it is used.
//!
//! Diffuse seals its flows and pictures on disk (`proton_diffuse::seal`);
//! the key is 32 bytes kept in the person's login keychain, as a password
//! item of Proton's own. Before the key is read, the person proves they are
//! at the Mac — Touch ID, or the Mac's password — through LocalAuthentication,
//! each time Diffuse is unlocked and each time its Gallery is opened.
//!
//! Why not a keychain item that asks for Touch ID by itself: that needs the
//! data-protection keychain, which an ad-hoc-signed app such as this one is
//! not let into. So the asking is LocalAuthentication's, and the keychain's
//! own guard is the usual one: an app other than the Proton that made the
//! item — a newer build of it too — must be allowed by the person, once,
//! with their password.

/// The keychain item's service and account.
#[cfg(target_os = "macos")]
const SERVICE: &str = "Proton Diffuse";
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "seal key";

/// `errSecItemNotFound`.
#[cfg(target_os = "macos")]
const NOT_FOUND: i32 = -25300;

/// Ask the person to prove they are at the Mac: Touch ID, or the Mac's
/// password. macOS says "Proton is trying to <reason>." Blocks until they
/// answer, so it is called off the main thread.
#[cfg(target_os = "macos")]
pub fn ask(reason: &str) -> Result<(), String> {
    use std::ffi::{c_char, CStr, CString};
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{class, msg_send};

    #[link(name = "LocalAuthentication", kind = "framework")]
    extern "C" {}

    /// `LAPolicyDeviceOwnerAuthentication`: biometrics, or the password.
    const OWNER: isize = 2;

    let Some(la) = AnyClass::get(c"LAContext") else {
        return Err("this Mac has no LocalAuthentication".into());
    };
    let text = CString::new(reason).map_err(|_| "a reason with a NUL in it")?;
    let (tell, told) = channel::<Result<(), String>>();
    // SAFETY: LAContext and NSString are messaged as their headers declare
    // them; the reply block is copied by LocalAuthentication and called once,
    // on a queue of its own, with a BOOL and an NSError or nil.
    unsafe {
        let context: Retained<AnyObject> = msg_send![la, new];
        let why: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()];
        let reply = RcBlock::new(move |ok: Bool, error: *mut AnyObject| {
            let said = if ok.as_bool() {
                Ok(())
            } else if error.is_null() {
                Err("not unlocked".to_string())
            } else {
                let code: isize = msg_send![error, code];
                let words: *mut AnyObject = msg_send![error, localizedDescription];
                let utf8: *const c_char = if words.is_null() { std::ptr::null() } else { msg_send![words, UTF8String] };
                let words =
                    if utf8.is_null() { String::new() } else { CStr::from_ptr(utf8).to_string_lossy().into_owned() };
                // Cancelled by the person, by the system, or by the app.
                Err(if matches!(code, -2 | -4 | -9) {
                    "not unlocked: cancelled".to_string()
                } else {
                    format!("not unlocked: {words}")
                })
            };
            let _ = tell.send(said);
        });
        let _: () = msg_send![&*context, evaluatePolicy: OWNER, localizedReason: why, reply: &*reply];
        // Kept until it has answered.
        let said = told.recv_timeout(Duration::from_secs(300)).unwrap_or(Err("not unlocked: no answer".into()));
        drop(context);
        said
    }
}

/// The key in the person's keychain, if there is one.
#[cfg(target_os = "macos")]
pub fn key() -> Result<Option<[u8; 32]>, String> {
    match security_framework::passwords::get_generic_password(SERVICE, ACCOUNT) {
        Ok(bytes) => {
            let key: [u8; 32] =
                bytes.as_slice().try_into().map_err(|_| "the keychain's Diffuse key is not 32 bytes".to_string())?;
            Ok(Some(key))
        }
        Err(e) if e.code() == NOT_FOUND => Ok(None),
        Err(e) => Err(format!("the keychain did not give Diffuse's key: {e}")),
    }
}

/// Keep a new key in the person's keychain.
#[cfg(target_os = "macos")]
pub fn keep(key: &[u8; 32]) -> Result<(), String> {
    security_framework::passwords::set_generic_password(SERVICE, ACCOUNT, key)
        .map_err(|e| format!("the keychain did not keep Diffuse's key: {e}"))
}

#[cfg(not(target_os = "macos"))]
pub fn ask(_reason: &str) -> Result<(), String> {
    Err("sealing Diffuse needs a Mac".into())
}

#[cfg(not(target_os = "macos"))]
pub fn key() -> Result<Option<[u8; 32]>, String> {
    Err("sealing Diffuse needs a Mac".into())
}

#[cfg(not(target_os = "macos"))]
pub fn keep(_key: &[u8; 32]) -> Result<(), String> {
    Err("sealing Diffuse needs a Mac".into())
}
