//! The WebAssembly's memory is capped, so a canvas too large for it fails
//! with a reason instead of growing until the window is killed.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        // 1 GiB, in bytes: a whole number of 64 KiB pages.
        println!("cargo:rustc-cdylib-link-arg=--max-memory=1073741824");
    }
}
