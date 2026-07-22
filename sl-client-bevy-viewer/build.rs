//! Build script: give the viewer's binaries (viewer, gallery, scenes) an
//! `$ORIGIN` rpath so they find `libcef.so` and the other CEF runtime files,
//! which the `cef-dll-sys` build script copies next to them into the cargo
//! target directory.

/// Emits the `$ORIGIN` rpath link argument for this crate's binaries.
fn main() {
    println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
}
