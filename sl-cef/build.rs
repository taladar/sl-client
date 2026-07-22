//! Build script: give the crate's binaries (the `sl-cef-helper` subprocess
//! executable) an `$ORIGIN` rpath so they find `libcef.so`, which the
//! `cef-dll-sys` build script copies next to them into the cargo target
//! directory.

/// Emits the `$ORIGIN` rpath link argument for this crate's binaries.
fn main() {
    println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
}
