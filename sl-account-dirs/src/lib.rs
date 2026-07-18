//! Per-avatar on-disk directory layout for a Second Life / OpenSim client.
//!
//! An avatar identity is **(grid, name)**: the same avatar name on SL's Agni,
//! the Aditi beta grid, and an OpenSim grid are three different avatars, and
//! Aditi is periodically cloned from Agni so the agent UUID alone is not
//! grid-unique — the grid must always appear in the path.
//!
//! The per-avatar directory is keyed by **name** (readable, and known before
//! login). The agent **UUID** is recorded as a reverse-index symlink so a paid
//! Linden name change is *discovered* on the next login and the readable
//! directory renamed, rather than the old data orphaned under the former name:
//!
//! ```text
//! <base>/<grid>/<name>/                    the per-avatar directory (canonical)
//! <base>/<grid>/.by-uuid/<uuid> -> <name>  reverse index, for rename discovery
//! ```
//!
//! [`reconcile_account_dir`] is the one entry point: given the accounts base
//! directory, the grid, the current login name, and the agent UUID (all known by
//! the moment the login response is parsed, before any per-avatar file is
//! touched), it creates or renames the directory and returns its path. It is
//! idempotent, so both a settings loader and a chat-log shell can call it.
//!
//! The crate does **not** choose the base directory — the host application does
//! (e.g. an XDG data dir via the `directories` crate).

use std::io;
use std::path::{Path, PathBuf};

use uuid::Uuid;

/// The reverse-index subdirectory under a grid, holding one entry per known
/// avatar UUID that points at that avatar's current name directory.
const BY_UUID_DIR: &str = ".by-uuid";

/// The placeholder used when a segment sanitises to nothing (an empty or
/// all-dots name / host).
const UNKNOWN_SEGMENT: &str = "unknown";

/// The filesystem-safe directory-name segment for a grid, derived from its login
/// URI: the host, with `:port` appended when the URI carries an explicit port
/// (so `login.agni.lindenlab.com` and `127.0.0.1:9000` are distinct grids). The
/// colon is kept — every target filesystem handles it in a path component.
///
/// A URI with no host (unusual for a login URI) falls back to a sanitised form
/// of the whole URI.
#[must_use]
pub fn grid_dir_name(login_uri: &url::Url) -> String {
    match login_uri.host_str() {
        Some(host) => match login_uri.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        },
        None => sanitize_segment(login_uri.as_str()),
    }
}

/// The filesystem-safe directory-name segment for an avatar, `First Last` (or
/// just `First` when the last name is empty), keeping the readable display form.
#[must_use]
pub fn avatar_dir_name(first: &str, last: &str) -> String {
    let joined = if last.trim().is_empty() {
        first.to_owned()
    } else {
        format!("{first} {last}")
    };
    sanitize_segment(&joined)
}

/// Resolve — and reconcile — the on-disk directory for one avatar, returning
/// `<base>/<grid>/<name>`.
///
/// Creates the directory (and the grid's reverse index) if absent. If the
/// reverse index shows `agent_uuid` previously lived under a *different* name (a
/// paid name change), the old directory is renamed to the current name and the
/// index repointed, so the avatar's settings / logs / caches follow the rename
/// instead of being orphaned. Idempotent: a steady-state login is a no-op that
/// just returns the path.
///
/// `grid` and `name` should come from [`grid_dir_name`] / [`avatar_dir_name`].
///
/// # Errors
///
/// Propagates any filesystem error from creating, reading, renaming, or linking
/// the directories.
pub fn reconcile_account_dir(
    base: &Path,
    grid: &str,
    name: &str,
    agent_uuid: Uuid,
) -> io::Result<PathBuf> {
    let grid_dir = base.join(grid);
    let by_uuid_dir = grid_dir.join(BY_UUID_DIR);
    let name_dir = grid_dir.join(name);
    let index_entry = by_uuid_dir.join(agent_uuid.to_string());

    // Ensures both the grid directory and its reverse index exist.
    fs_err::create_dir_all(&by_uuid_dir)?;

    match read_index(&index_entry)? {
        // Known UUID, but under a different name → a rename was discovered.
        Some(previous) if previous != name => {
            let previous_dir = grid_dir.join(&previous);
            // Migrate the old directory to the new name, unless the destination
            // already exists (never clobber existing data).
            if previous_dir.exists() && !name_dir.exists() {
                fs_err::rename(&previous_dir, &name_dir)?;
            }
            write_index(&index_entry, name)?;
        }
        // Known UUID already under this name → nothing to do.
        Some(_current) => {}
        // First time this UUID is seen on this grid → record it.
        None => write_index(&index_entry, name)?,
    }

    fs_err::create_dir_all(&name_dir)?;
    Ok(name_dir)
}

/// Read the name a reverse-index entry points at, or `None` if the entry does
/// not exist.
fn read_index(entry: &Path) -> io::Result<Option<String>> {
    match read_index_target(entry) {
        Ok(name) => Ok(Some(name)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Write (or overwrite) a reverse-index entry so it points at `name`.
fn write_index(entry: &Path, name: &str) -> io::Result<()> {
    // Overwrite: an existing entry (a rename repoint) must be removed first.
    match remove_index(entry) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    write_index_target(entry, name)
}

/// Remove a reverse-index entry.
fn remove_index(entry: &Path) -> io::Result<()> {
    fs_err::remove_file(entry)
}

#[cfg(unix)]
/// The name a reverse-index symlink points at (its `../<name>` target's final
/// component).
fn read_index_target(entry: &Path) -> io::Result<String> {
    let target = fs_err::read_link(entry)?;
    target
        .file_name()
        .and_then(|component| component.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "reverse-index target has no name",
            )
        })
}

#[cfg(unix)]
/// Create a reverse-index symlink pointing at the sibling `../<name>` directory,
/// so browsing `.by-uuid/` resolves each UUID to its readable name directory.
fn write_index_target(entry: &Path, name: &str) -> io::Result<()> {
    let target = Path::new("..").join(name);
    fs_err::os::unix::fs::symlink(target, entry)
}

#[cfg(not(unix))]
/// The name a reverse-index file records (its contents), on platforms without a
/// reliable symlink (a plain file is used instead of a symlink there).
fn read_index_target(entry: &Path) -> io::Result<String> {
    Ok(fs_err::read_to_string(entry)?.trim().to_owned())
}

#[cfg(not(unix))]
/// Record the name in a reverse-index file, on platforms without a reliable
/// symlink.
fn write_index_target(entry: &Path, name: &str) -> io::Result<()> {
    fs_err::write(entry, name)
}

/// Map a string to a single filesystem-safe path component: keep letters,
/// digits, space, `.`, `-`, `_`; replace anything else with `_`; and fall back
/// to [`UNKNOWN_SEGMENT`] for an empty or all-dots result (which would clash with
/// `.`/`..`).
fn sanitize_segment(raw: &str) -> String {
    let mapped: String = raw
        .trim()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if mapped.is_empty() || mapped.chars().all(|character| character == '.') {
        UNKNOWN_SEGMENT.to_owned()
    } else {
        mapped
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::{avatar_dir_name, grid_dir_name, reconcile_account_dir};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A unique temporary base directory, namespaced by crate + test thread so
    /// parallel `nextest` binaries never share a path.
    fn tempdir() -> Result<std::path::PathBuf, TestError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "{}-accounts-{nanos}-{:?}",
            env!("CARGO_PKG_NAME"),
            std::thread::current().id()
        ));
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// The grid segment is the host, with an explicit port appended.
    #[test]
    fn grid_dir_name_uses_host_and_explicit_port() -> Result<(), TestError> {
        assert_eq!(
            grid_dir_name(&url::Url::parse(
                "https://login.agni.lindenlab.com/cgi-bin/login.cgi"
            )?),
            "login.agni.lindenlab.com"
        );
        assert_eq!(
            grid_dir_name(&url::Url::parse("http://127.0.0.1:9000/")?),
            "127.0.0.1:9000"
        );
        Ok(())
    }

    /// The avatar segment is the readable `First Last`, or just `First` for a
    /// single-name account, with unsafe characters replaced.
    #[test]
    fn avatar_dir_name_is_readable_and_safe() {
        assert_eq!(avatar_dir_name("Alice", "Resident"), "Alice Resident");
        assert_eq!(avatar_dir_name("Bob", ""), "Bob");
        assert_eq!(avatar_dir_name("a/b", "c\\d"), "a_b c_d");
    }

    /// A first login creates the per-avatar directory; a second identical login
    /// returns the same path and changes nothing.
    #[test]
    fn first_login_creates_and_is_idempotent() -> Result<(), TestError> {
        let base = tempdir()?;
        let uuid = uuid::Uuid::from_u128(1);
        let first = reconcile_account_dir(&base, "grid.example:9000", "Alice Resident", uuid)?;
        assert!(first.is_dir());
        assert!(first.ends_with("Alice Resident"));

        let again = reconcile_account_dir(&base, "grid.example:9000", "Alice Resident", uuid)?;
        assert_eq!(first, again);
        assert!(again.is_dir());
        Ok(())
    }

    /// A login under a new name with a UUID last seen under an old name renames
    /// the directory in place (a paid Linden name change), carrying its contents.
    #[test]
    fn rename_discovered_moves_directory() -> Result<(), TestError> {
        let base = tempdir()?;
        let uuid = uuid::Uuid::from_u128(2);
        let old = reconcile_account_dir(&base, "agni", "Old Name", uuid)?;
        // Drop a file so we can prove the contents move with the rename.
        fs_err::write(old.join("settings.toml"), "marker = true\n")?;

        let new = reconcile_account_dir(&base, "agni", "New Name", uuid)?;
        assert!(new.ends_with("New Name"));
        assert!(new.is_dir());
        // The old directory is gone and the file moved into the new one.
        assert!(!old.exists());
        assert_eq!(
            fs_err::read_to_string(new.join("settings.toml"))?,
            "marker = true\n"
        );
        Ok(())
    }

    /// The same avatar name on two different grids resolves to two distinct
    /// directories (grid is always in the path).
    #[test]
    fn same_name_on_two_grids_is_distinct() -> Result<(), TestError> {
        let base = tempdir()?;
        let agni =
            reconcile_account_dir(&base, "agni", "Alice Resident", uuid::Uuid::from_u128(3))?;
        let aditi =
            reconcile_account_dir(&base, "aditi", "Alice Resident", uuid::Uuid::from_u128(3))?;
        assert_ne!(agni, aditi);
        assert!(agni.is_dir());
        assert!(aditi.is_dir());
        Ok(())
    }
}
