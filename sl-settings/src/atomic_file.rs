//! Replacing a file on disk without ever leaving it half-written, and setting
//! an unreadable one aside instead of destroying it.
//!
//! # Why this is not `fs::write`
//!
//! `fs_err::write` opens with `O_TRUNC`: the moment it returns from `open` the
//! old contents are gone, and everything after that is a window in which a
//! crash, a full disk or a killed process leaves a **truncated or empty** file
//! where a good one used to be. For a cache that is a nuisance; for a store
//! that is the only copy of something the user has not answered yet — settings,
//! open notifications — it is data loss, and it is the mechanism behind
//! `viewer-audit-settings-write-race` and
//! `viewer-audit-notification-store-overwrite`.
//!
//! [`write_atomically`] instead writes a sibling temporary file, flushes it to
//! the platform, and `rename`s it over the target. A rename within one
//! directory is atomic on every platform the viewer runs on, so a reader sees
//! either the whole old file or the whole new one and never a partial write.
//!
//! # Why the temporary file is a *sibling*
//!
//! `rename` is only atomic — only *possible*, on Unix — within a filesystem, so
//! the temporary is created next to its target rather than in the system temp
//! directory, which is routinely a different mount.
//!
//! # The other half: not writing at all
//!
//! Atomicity protects a file from a write that fails halfway. It does nothing
//! about a write that should never have happened, which is the more common way
//! a store is lost: something fails to *parse* the file, treats "I could not
//! read it" as "it was empty", and the next flush writes that emptiness back
//! over the real data. [`move_aside`] is the answer to that — preserve the
//! unreadable file under a new name, so a later flush cannot reach it and a
//! human (or a support request) still can.

use std::io;
use std::path::{Path, PathBuf};

/// The extension given to a temporary file while it is being written.
const TEMP_EXTENSION: &str = "tmp";

/// The extension given to a file that could not be read or parsed.
const CORRUPT_EXTENSION: &str = "corrupt";

/// Write `contents` to `path`, replacing it atomically.
///
/// The new bytes go to a sibling temporary file which is flushed and then
/// renamed over `path`, so a concurrent reader sees the whole old file or the
/// whole new one. `path`'s parent directory must exist.
///
/// On any failure the temporary file is removed and `path` is left exactly as
/// it was — which is the point: a caller that cannot write must not thereby
/// destroy what is already there.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] when the parent directory cannot be
/// determined, or when creating, writing, flushing or renaming the temporary
/// file fails.
pub fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no parent directory to write into", path.display()),
        )
    })?;
    let temp = unique_sibling(path, TEMP_EXTENSION);

    // Scoped so the handle is dropped — and on Windows the file closed — before
    // the rename. `sync_all` is what makes "flushed" mean flushed to the
    // platform rather than to a buffer in this process.
    let written = (|| -> io::Result<()> {
        use std::io::Write as _;
        let file = fs_err::File::create(&temp)?;
        let mut file = io::BufWriter::new(file);
        file.write_all(contents.as_bytes())?;
        file.flush()?;
        file.into_inner()
            .map_err(io::IntoInnerError::into_error)?
            .sync_all()?;
        Ok(())
    })();
    if let Err(error) = written {
        // Best-effort: the write already failed, and failing to clean up after
        // it must not replace that error with a less useful one.
        drop(fs_err::remove_file(&temp));
        return Err(error);
    }

    if let Err(error) = fs_err::rename(&temp, path) {
        drop(fs_err::remove_file(&temp));
        return Err(error);
    }

    // The rename is durable only once the *directory* entry is, which matters
    // for the crash this whole function exists for. Unix only: Windows has no
    // handle to a directory to sync, and its rename is already ordered.
    #[cfg(unix)]
    if let Ok(dir) = fs_err::File::open(parent) {
        // Best-effort. A filesystem that refuses to sync a directory handle
        // (some network mounts do) has still taken the rename.
        drop(dir.sync_all());
    }
    #[cfg(not(unix))]
    let _ = parent;

    Ok(())
}

/// Rename `path` out of the way, returning where it went.
///
/// For a file that exists but could not be read or parsed: the caller keeps
/// working with an empty state, and the bytes it could not understand survive
/// under a neighbouring name instead of being overwritten by the next flush.
///
/// The new name carries a timestamp, so a second corruption does not destroy
/// the evidence from the first.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] when the rename fails — in which case
/// the caller has *not* protected the file and must not write over it either.
pub fn move_aside(path: &Path) -> io::Result<PathBuf> {
    let aside = unique_sibling(path, CORRUPT_EXTENSION);
    fs_err::rename(path, &aside)?;
    Ok(aside)
}

/// A path next to `path` with `extension` and a timestamp appended, e.g.
/// `open_notifications.json.corrupt-1756000000000000000`.
///
/// The timestamp is nanoseconds since the epoch, which is enough to keep two
/// files written in the same session apart; a clock before the epoch (or one
/// that has been stepped backwards) falls back to `0` rather than failing,
/// since a colliding name is a far smaller problem than refusing to protect the
/// file at all.
fn unique_sibling(path: &Path, extension: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{extension}-{nanos}"));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::{move_aside, write_atomically};
    use pretty_assertions::{assert_eq, assert_ne};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A unique throwaway directory under the system temp dir (the crate has no
    /// `tempfile` dependency).
    fn tempdir(label: &str) -> Result<std::path::PathBuf, TestError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "{}-{label}-{nanos}-{:?}",
            env!("CARGO_PKG_NAME"),
            std::thread::current().id()
        ));
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// **The replacement is complete, and leaves nothing behind.**
    ///
    /// Both halves matter. A reader must never find the old contents after a
    /// successful write, and the directory must not silently fill with the
    /// temporary files every write creates — a leak that would only show up
    /// after months of a user's settings being saved.
    #[test]
    fn a_write_replaces_the_file_and_leaves_no_temporary() -> Result<(), TestError> {
        let dir = tempdir("atomic-write")?;
        let path = dir.join("store.json");
        fs_err::write(&path, "the old contents")?;

        write_atomically(&path, "the new contents")?;
        assert_eq!(fs_err::read_to_string(&path)?, "the new contents");

        // Creating the file where none existed works the same way.
        let fresh = dir.join("fresh.json");
        write_atomically(&fresh, "first")?;
        assert_eq!(fs_err::read_to_string(&fresh)?, "first");

        let strays: Vec<String> = fs_err::read_dir(&dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert_eq!(
            strays,
            Vec::<String>::new(),
            "temporary files were left behind"
        );
        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }

    /// **A failed write does not destroy what is already there.**
    ///
    /// The reason to prefer this over `fs::write` at all: `O_TRUNC` empties the
    /// file before it can discover it cannot finish. Here the target is a
    /// *directory*, so the rename is guaranteed to fail after the temporary has
    /// been written — and the existing entry has to survive it intact.
    #[test]
    fn a_failed_write_leaves_the_original_intact() -> Result<(), TestError> {
        let dir = tempdir("atomic-fail")?;
        let occupied = dir.join("store.json");
        fs_err::create_dir_all(&occupied)?;
        fs_err::write(occupied.join("inside"), "still here")?;

        assert!(
            write_atomically(&occupied, "clobber").is_err(),
            "renaming over a non-empty directory must fail"
        );
        assert_eq!(
            fs_err::read_to_string(occupied.join("inside"))?,
            "still here",
            "the failed write destroyed the target"
        );
        let strays: Vec<String> = fs_err::read_dir(&dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert_eq!(
            strays,
            Vec::<String>::new(),
            "a failed write left its temporary behind"
        );
        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }

    /// **A file moved aside is preserved, not deleted — and twice over.**
    ///
    /// The second corruption is the case worth pinning: a fixed `.corrupt` name
    /// would quietly overwrite the first one's evidence, which is the same
    /// class of mistake as the bug this module exists to prevent.
    #[test]
    fn moving_aside_preserves_every_copy() -> Result<(), TestError> {
        let dir = tempdir("aside")?;
        let path = dir.join("store.json");

        fs_err::write(&path, "first corruption")?;
        let first = move_aside(&path)?;
        assert!(!path.exists(), "the unreadable file is out of the way");
        assert_eq!(fs_err::read_to_string(&first)?, "first corruption");

        fs_err::write(&path, "second corruption")?;
        let second = move_aside(&path)?;
        assert_ne!(first, second, "the second rescue overwrote the first");
        assert_eq!(fs_err::read_to_string(&first)?, "first corruption");
        assert_eq!(fs_err::read_to_string(&second)?, "second corruption");

        // Nothing to move: the caller learns so rather than being told it
        // succeeded — which for a caller of this function is the difference
        // between "the file is safe" and "there was never a file".
        if let Ok(nowhere) = move_aside(&dir.join("absent.json")) {
            return Err(
                format!("moving aside a file that does not exist reported {nowhere:?}").into(),
            );
        }
        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }
}
