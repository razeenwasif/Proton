fn main() {
    // `zmq-sys` vendors libzmq and builds it, which is why no system library
    // has to be installed. What it does not do is ask for the Win32 security
    // APIs libzmq's TCP socketpair emulation calls, so linking fails on MSVC
    // with two unresolved symbols — `InitializeSecurityDescriptor` and
    // `SetSecurityDescriptorDacl`. Both live in advapi32.
    //
    // Worth knowing how this hides: a crate that merely *depends* on `zmq`
    // links fine, because nothing references those objects until something
    // actually opens a socket. It first fails at the point the code starts
    // working.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=advapi32");
    }
}
