//! Embeds `Info.plist` into the `__TEXT,__info_plist` section of the binary.
//!
//! macOS uses this embedded plist to give a bare command-line executable a
//! bundle identity, which is required for CoreLocation authorization. Without
//! Location Services authorization, CoreWLAN redacts SSIDs/BSSIDs.

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let plist = format!("{dir}/Info.plist");
    println!("cargo:rerun-if-changed=Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{plist}");
    }
}
