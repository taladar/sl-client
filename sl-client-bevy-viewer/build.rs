//! Build script: gives the viewer's binaries (viewer, gallery, scenes) an
//! `$ORIGIN` rpath so they find `libcef.so` and the other CEF runtime files,
//! and embeds build-time metadata for the About floater — the `git describe`
//! version string and the Bevy / wgpu dependency versions from `Cargo.lock`.

/// Emits the rpath link argument and the build-metadata `rustc-env`s.
///
/// # Errors
///
/// Fails if the workspace `Cargo.lock` cannot be read; the git metadata is
/// best-effort (a source tarball build simply has no `SL_VIEWER_GIT_DESCRIBE`).
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    println!("cargo::rerun-if-changed=build.rs");

    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    emit_git_describe(&manifest_dir);
    emit_dependency_versions(&manifest_dir)?;
    Ok(())
}

/// Runs a git command in `dir` and returns its trimmed stdout, or `None` when
/// git is unavailable, the directory is not a checkout, or the output is not
/// UTF-8 — all of which downgrade to "no git metadata" rather than a build
/// failure.
fn git_stdout(dir: &std::path::Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Emits `SL_VIEWER_GIT_DESCRIBE` from `git describe --tags --always --dirty`
/// plus `rerun-if-changed` hints on the checkout's `HEAD` (and the branch ref
/// it points at) so the value refreshes on commit / branch switch. Best-effort:
/// outside a git checkout nothing is emitted and the viewer falls back to the
/// crate version alone.
fn emit_git_describe(manifest_dir: &std::path::Path) {
    let Some(git_dir) = git_stdout(manifest_dir, &["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    let Some(describe) = git_stdout(manifest_dir, &["describe", "--tags", "--always", "--dirty"])
    else {
        return;
    };
    println!("cargo::rustc-env=SL_VIEWER_GIT_DESCRIBE={describe}");
    // `--absolute-git-dir` resolves the per-worktree git dir (this repo is used
    // via `git worktree`), so `HEAD` here is the right file to watch.
    let head = std::path::PathBuf::from(&git_dir).join("HEAD");
    if head.exists() {
        println!("cargo::rerun-if-changed={}", head.display());
        // A symbolic `HEAD` names the branch ref that moves on every commit;
        // refs live under the *common* git dir, shared across worktrees.
        if let Ok(content) = fs_err::read_to_string(&head)
            && let Some(reference) = content.trim().strip_prefix("ref: ")
            && let Some(common) = git_stdout(manifest_dir, &["rev-parse", "--git-common-dir"])
        {
            let ref_path = std::path::PathBuf::from(common).join(reference);
            if ref_path.exists() {
                println!("cargo::rerun-if-changed={}", ref_path.display());
            }
        }
    }
}

/// Emits `SL_VIEWER_BEVY_VERSION` / `SL_VIEWER_WGPU_VERSION` parsed from the
/// workspace `Cargo.lock`, for the About floater's library-versions line.
///
/// # Errors
///
/// Fails if `Cargo.lock` cannot be read — it is committed, so a missing file
/// is a broken checkout rather than an expected state.
fn emit_dependency_versions(
    manifest_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let lock_path = manifest_dir
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent directory")?
        .join("Cargo.lock");
    println!("cargo::rerun-if-changed={}", lock_path.display());
    let lock = fs_err::read_to_string(&lock_path)?;
    for (package, env) in [
        ("bevy", "SL_VIEWER_BEVY_VERSION"),
        ("wgpu", "SL_VIEWER_WGPU_VERSION"),
    ] {
        if let Some(version) = locked_version(&lock, package) {
            println!("cargo::rustc-env={env}={version}");
        }
    }
    Ok(())
}

/// Returns the `version` recorded in `Cargo.lock` for the exactly-named
/// `package` (the first match; the workspace locks a single version of each).
fn locked_version(lock: &str, package: &str) -> Option<String> {
    let mut in_package = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            in_package = false;
            continue;
        }
        if let Some(name) = line
            .strip_prefix("name = \"")
            .and_then(|rest| rest.strip_suffix('"'))
        {
            in_package = name == package;
            continue;
        }
        if in_package
            && let Some(version) = line
                .strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
        {
            return Some(version.to_owned());
        }
    }
    None
}
