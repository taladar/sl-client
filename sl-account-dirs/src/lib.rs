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
//! touched), it creates or renames the directory and returns its path together
//! with a [`Reconciliation`] saying what it did — including the case where the
//! new name is already taken and the rename had to be abandoned. It is
//! idempotent, so both a settings loader and a chat-log shell can call it.
//!
//! The crate does **not** choose the base directory — the host application does
//! (e.g. an XDG data dir via the `directories` crate).

use std::ffi::OsStr;
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

/// How [`reconcile_account_dir`] resolved one avatar's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconciliation {
    /// The directory carries the login name: a first login, or the steady state
    /// where nothing had to move.
    Settled,
    /// A name change was discovered and the directory was moved out of
    /// `previous`, so the avatar's data followed the rename.
    Renamed {
        /// The name the directory carried before the move.
        previous: String,
    },
    /// A name change was discovered, but the login name is already taken on
    /// this grid — by a directory on disk, or by another avatar's reverse-index
    /// entry. Nothing was moved and the index was left pointing at `previous`,
    /// so this avatar keeps its own data and no other avatar's data is handed
    /// to it. Resolving the collision needs a human: the directory holding the
    /// name has to be renamed or removed.
    NameTaken {
        /// The name this avatar's directory still carries.
        previous: String,
    },
}

impl Reconciliation {
    /// The message a caller should log when reconciliation could not put the
    /// avatar under its login `name`, or `None` when it settled cleanly. Shared
    /// so every caller words the collision the same way; each prefixes it with
    /// its own subsystem.
    #[must_use]
    pub fn collision_warning(&self, name: &str) -> Option<String> {
        match self {
            Self::Settled | Self::Renamed { .. } => None,
            Self::NameTaken { previous } => Some(format!(
                "the account directory {name:?} is already taken by another avatar on this grid, \
                 so this avatar keeps its own data under {previous:?} — rename or remove the \
                 directory holding {name:?} to complete the name change"
            )),
        }
    }
}

/// One avatar's reconciled on-disk directory: the path to use, and how it was
/// arrived at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountDir {
    /// The directory holding this avatar's settings, logs and caches.
    pub path: PathBuf,
    /// What reconciliation did — in particular whether a discovered rename
    /// could not be carried out ([`Reconciliation::NameTaken`]), which a caller
    /// should report rather than swallow.
    pub outcome: Reconciliation,
}

/// Resolve — and reconcile — the on-disk directory for one avatar, normally
/// `<base>/<grid>/<name>`.
///
/// Creates the directory (and the grid's reverse index) if absent. If the
/// reverse index shows `agent_uuid` previously lived under a *different* name (a
/// paid name change), the old directory is renamed to the current name and the
/// index repointed, so the avatar's settings / logs / caches follow the rename
/// instead of being orphaned. Idempotent: a steady-state login is a no-op that
/// just returns the path.
///
/// The index is repointed **only** when the avatar really ends up under `name`.
/// When the name is already taken — a directory of that name exists, or another
/// UUID's index entry claims it — the rename is abandoned, the avatar stays
/// under its former name, and the returned [`AccountDir::outcome`] is
/// [`Reconciliation::NameTaken`]. Repointing the index there anyway would hand
/// this avatar the *other* avatar's settings, chat logs and inventory cache and
/// orphan its own.
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
) -> io::Result<AccountDir> {
    let grid_dir = base.join(grid);
    let by_uuid_dir = grid_dir.join(BY_UUID_DIR);
    let name_dir = grid_dir.join(name);
    let index_entry = by_uuid_dir.join(agent_uuid.to_string());

    // Ensures both the grid directory and its reverse index exist.
    fs_err::create_dir_all(&by_uuid_dir)?;

    let outcome = match read_index(&index_entry)? {
        // Known UUID, but under a different name → a rename was discovered.
        Some(previous) if previous != name => {
            // Never clobber existing data, and never adopt it either: the new
            // name is taken if anything already occupies it on disk or in the
            // index.
            if name_dir.exists() || name_claimed_by_other(&by_uuid_dir, name, agent_uuid)? {
                Reconciliation::NameTaken { previous }
            } else {
                let previous_dir = grid_dir.join(&previous);
                // The old directory can be gone (deleted by hand); then there
                // is nothing to migrate and only the index moves.
                let moved = previous_dir.exists();
                if moved {
                    fs_err::rename(&previous_dir, &name_dir)?;
                }
                write_index(&index_entry, name)?;
                if moved {
                    Reconciliation::Renamed { previous }
                } else {
                    Reconciliation::Settled
                }
            }
        }
        // Known UUID already under this name → nothing to do.
        Some(_current) => Reconciliation::Settled,
        // First time this UUID is seen on this grid → record it.
        None => {
            write_index(&index_entry, name)?;
            Reconciliation::Settled
        }
    };

    let path = match &outcome {
        // The avatar stays where its data is, under the former name.
        Reconciliation::NameTaken { previous } => grid_dir.join(previous),
        Reconciliation::Settled | Reconciliation::Renamed { .. } => name_dir,
    };
    fs_err::create_dir_all(&path)?;
    Ok(AccountDir { path, outcome })
}

/// Whether some *other* avatar's reverse-index entry on this grid already claims
/// `name` — true even when that avatar's directory has since been deleted, since
/// repointing the name to this avatar would silently steal it at that avatar's
/// next login.
fn name_claimed_by_other(by_uuid_dir: &Path, name: &str, agent_uuid: Uuid) -> io::Result<bool> {
    let own_entry = agent_uuid.to_string();
    for entry in fs_err::read_dir(by_uuid_dir)? {
        let entry = entry?;
        if entry.file_name() == OsStr::new(own_entry.as_str()) {
            continue;
        }
        if read_index(&entry.path())?.is_some_and(|claimed| claimed == name) {
            return Ok(true);
        }
    }
    Ok(false)
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

    use super::{Reconciliation, avatar_dir_name, grid_dir_name, reconcile_account_dir};

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
        assert!(first.path.is_dir());
        assert!(first.path.ends_with("Alice Resident"));
        assert_eq!(first.outcome, Reconciliation::Settled);

        let again = reconcile_account_dir(&base, "grid.example:9000", "Alice Resident", uuid)?;
        assert_eq!(first, again);
        assert!(again.path.is_dir());
        Ok(())
    }

    /// A login under a new name with a UUID last seen under an old name renames
    /// the directory in place (a paid Linden name change), carrying its contents.
    #[test]
    fn rename_discovered_moves_directory() -> Result<(), TestError> {
        let base = tempdir()?;
        let uuid = uuid::Uuid::from_u128(2);
        let old = reconcile_account_dir(&base, "agni", "Old Name", uuid)?.path;
        // Drop a file so we can prove the contents move with the rename.
        fs_err::write(old.join("settings.toml"), "marker = true\n")?;

        let new = reconcile_account_dir(&base, "agni", "New Name", uuid)?;
        assert_eq!(
            new.outcome,
            Reconciliation::Renamed {
                previous: "Old Name".to_owned()
            }
        );
        assert!(new.path.ends_with("New Name"));
        assert!(new.path.is_dir());
        // The old directory is gone and the file moved into the new one.
        assert!(!old.exists());
        assert_eq!(
            fs_err::read_to_string(new.path.join("settings.toml"))?,
            "marker = true\n"
        );
        Ok(())
    }

    /// A name change into a name another avatar already occupies is refused:
    /// the renaming avatar keeps its own data under its former name, the
    /// occupant keeps its directory, and neither is handed the other's files.
    #[test]
    fn rename_into_an_occupied_name_is_refused() -> Result<(), TestError> {
        let base = tempdir()?;
        let mover = uuid::Uuid::from_u128(4);
        let occupant = uuid::Uuid::from_u128(5);

        let mover_dir = reconcile_account_dir(&base, "agni", "Old Name", mover)?.path;
        fs_err::write(mover_dir.join("settings.toml"), "mover = true\n")?;
        let occupant_dir = reconcile_account_dir(&base, "agni", "New Name", occupant)?.path;
        fs_err::write(occupant_dir.join("settings.toml"), "occupant = true\n")?;

        let resolved = reconcile_account_dir(&base, "agni", "New Name", mover)?;
        assert_eq!(
            resolved.outcome,
            Reconciliation::NameTaken {
                previous: "Old Name".to_owned()
            }
        );
        // The mover stays on its own directory, with its own settings.
        assert_eq!(resolved.path, mover_dir);
        assert_eq!(
            fs_err::read_to_string(resolved.path.join("settings.toml"))?,
            "mover = true\n"
        );
        // And the occupant still owns its directory at its next login.
        let occupant_again = reconcile_account_dir(&base, "agni", "New Name", occupant)?;
        assert_eq!(occupant_again.path, occupant_dir);
        assert_eq!(occupant_again.outcome, Reconciliation::Settled);
        assert_eq!(
            fs_err::read_to_string(occupant_dir.join("settings.toml"))?,
            "occupant = true\n"
        );
        Ok(())
    }

    /// The name is taken even when the occupant's directory has been deleted by
    /// hand: its reverse-index entry still claims the name, and repointing the
    /// name would hand the mover's data to the occupant at its next login.
    #[test]
    fn rename_into_a_name_claimed_only_by_the_index_is_refused() -> Result<(), TestError> {
        let base = tempdir()?;
        let mover = uuid::Uuid::from_u128(6);
        let occupant = uuid::Uuid::from_u128(7);

        let mover_dir = reconcile_account_dir(&base, "agni", "Old Name", mover)?.path;
        fs_err::write(mover_dir.join("settings.toml"), "mover = true\n")?;
        let occupant_dir = reconcile_account_dir(&base, "agni", "New Name", occupant)?.path;
        fs_err::remove_dir_all(&occupant_dir)?;

        let resolved = reconcile_account_dir(&base, "agni", "New Name", mover)?;
        assert_eq!(
            resolved.outcome,
            Reconciliation::NameTaken {
                previous: "Old Name".to_owned()
            }
        );
        assert_eq!(resolved.path, mover_dir);
        // The occupant's fresh directory is its own, and empty.
        let occupant_again = reconcile_account_dir(&base, "agni", "New Name", occupant)?;
        assert_eq!(occupant_again.path, occupant_dir);
        assert!(!occupant_dir.join("settings.toml").exists());
        Ok(())
    }

    /// A rename whose former directory was deleted by hand still repoints the
    /// index — there is nothing to migrate, and the login name is free.
    #[test]
    fn rename_with_no_former_directory_repoints_the_index() -> Result<(), TestError> {
        let base = tempdir()?;
        let uuid = uuid::Uuid::from_u128(8);
        let old = reconcile_account_dir(&base, "agni", "Old Name", uuid)?.path;
        fs_err::remove_dir_all(&old)?;

        let new = reconcile_account_dir(&base, "agni", "New Name", uuid)?;
        assert!(new.path.ends_with("New Name"));
        assert!(new.path.is_dir());
        assert_eq!(new.outcome, Reconciliation::Settled);
        // The index followed, so the next login under the new name is a no-op.
        let again = reconcile_account_dir(&base, "agni", "New Name", uuid)?;
        assert_eq!(again.path, new.path);
        assert_eq!(again.outcome, Reconciliation::Settled);
        Ok(())
    }

    /// The same avatar name on two different grids resolves to two distinct
    /// directories (grid is always in the path).
    #[test]
    fn same_name_on_two_grids_is_distinct() -> Result<(), TestError> {
        let base = tempdir()?;
        let agni =
            reconcile_account_dir(&base, "agni", "Alice Resident", uuid::Uuid::from_u128(3))?.path;
        let aditi =
            reconcile_account_dir(&base, "aditi", "Alice Resident", uuid::Uuid::from_u128(3))?.path;
        assert_ne!(agni, aditi);
        assert!(agni.is_dir());
        assert!(aditi.is_dir());
        Ok(())
    }
}
