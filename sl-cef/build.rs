//! Build script: puts the CEF runtime files (`libcef.so`, the `.pak` files,
//! `icudtl.dat`, `chrome-sandbox`, `locales/`, …) beside the binaries in
//! `target/<profile>/`.
//!
//! `cef-dll-sys` copies them itself, but finds the profile directory by going
//! three levels up from its build-output directory. Cargo 1.100's build-dir
//! layout (`<profile>/build/<package>/<hash>/out`) has one level more, so the
//! files land in `<profile>/build/` instead, where no binary, test or example
//! finds them: every test binary that loads `libcef.so` fails to start. Every
//! crate whose binaries load it depends on `sl-cef`, so this script copies
//! them to the right place for all of them, from the distribution directory
//! `cef-dll-sys` publishes (`DEP_CEF_DLL_WRAPPER_CEF_DIR`).

use std::path::{Path, PathBuf};

/// The subdirectory of the distribution holding Chromium's locale `.pak`s.
const LOCALES_DIR: &str = "locales";

/// Copies the runtime files into the profile directory.
///
/// # Errors
///
/// Fails if `cef-dll-sys` published no distribution directory, if the profile
/// directory cannot be found from `OUT_DIR`, or if a copy fails.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rerun-if-changed=build.rs");
    // Only Linux and Windows run CEF from beside the binary; macOS wants an
    // app bundle, which this workspace does not build.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS")?;
    if target_os != "linux" && target_os != "windows" {
        return Ok(());
    }
    let cef_dir = PathBuf::from(
        std::env::var("DEP_CEF_DLL_WRAPPER_CEF_DIR")
            .map_err(|error| format!("cef-dll-sys published no CEF_DIR: {error}"))?,
    );
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let profile_dir = profile_dir(&out_dir).ok_or_else(|| {
        format!(
            "no `build` directory above OUT_DIR {}, so the profile directory is unknown",
            out_dir.display()
        )
    })?;
    println!("cargo::rerun-if-changed={}", cef_dir.display());
    copy_files(&cef_dir, profile_dir)?;
    copy_files(&cef_dir.join(LOCALES_DIR), &profile_dir.join(LOCALES_DIR))?;
    Ok(())
}

/// The profile directory (`target/<profile>/`) for a build script's `OUT_DIR`:
/// the parent of the nearest ancestor named `build`, which is
/// `<profile>/build/<package>-<hash>/out` in cargo's old layout and
/// `<profile>/build/<package>/<hash>/out` in 1.100's.
fn profile_dir(out_dir: &Path) -> Option<&Path> {
    out_dir
        .ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "build"))
        .and_then(Path::parent)
}

/// Copies every regular file directly in `source` into `destination`.
///
/// A copy is given its source's modification time, so a file already there
/// with the same length and time is taken as unchanged and skipped. Each
/// destination is a `rerun-if-changed`: a deleted one runs the script again
/// on the next build, and since its time is the source's, which is older than
/// this run, a copy does not count as a change by itself.
///
/// # Errors
///
/// Fails if a directory cannot be read or created, or a file copied.
fn copy_files(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs_err::create_dir_all(destination)?;
    for entry in fs_err::read_dir(source)? {
        let entry = entry?;
        let metadata = fs_err::metadata(entry.path())?;
        if !metadata.is_file() {
            continue;
        }
        let target = destination.join(entry.file_name());
        println!("cargo::rerun-if-changed={}", target.display());
        let modified = metadata.modified()?;
        let unchanged = fs_err::metadata(&target).is_ok_and(|existing| {
            existing.len() == metadata.len()
                && existing.modified().is_ok_and(|time| time == modified)
        });
        if unchanged {
            continue;
        }
        fs_err::copy(entry.path(), &target)?;
        fs_err::File::options()
            .write(true)
            .open(&target)?
            .set_modified(modified)?;
    }
    Ok(())
}
