//! The viewer's on-disk locations, resolved through the platform's standard
//! directories (`directories` crate: XDG on Linux, the equivalents elsewhere).
//!
//! Each kind of persistence lands under the XDG root that fits its category, so
//! a per-avatar `accounts/<grid>/<name>/` tree exists independently under three
//! roots (each keyed by grid + avatar name with UUID rename discovery — see
//! [`sl_account_dirs`]):
//!
//! - **config** (`~/.config/sl-client-bevy-viewer`) — the machine-wide
//!   [`Global`](sl_settings::Scope::Global) settings file, and the per-avatar
//!   [`Account`](sl_settings::Scope::Account) settings under
//!   [`config_accounts_base`].
//! - **state** (`~/.local/state/sl-client-bevy-viewer`) — the per-avatar chat
//!   transcripts under [`state_accounts_base`] (user-facing log state).
//! - **cache** (`~/.cache/sl-client-bevy-viewer`) — the content-addressed asset
//!   caches (textures / meshes / materials / animations / bake inputs), keyed by
//!   asset UUID and shared across every avatar and grid; plus the per-avatar,
//!   regenerable inventory cache under [`cache_accounts_base`].
//!
//! The cache root matches the location the asset caches used before this module
//! (`$XDG_CACHE_HOME`/`~/.cache` + `sl-client-bevy-viewer`), so moving them onto
//! the `directories` crate does not invalidate an existing cache.

use std::path::PathBuf;
use std::sync::OnceLock;

use directories::ProjectDirs;

/// A process-global override for the asset-cache root, set once by the
/// avatar-state **replay** mode ([`crate::avatar_replay`]) before the app is
/// built. When set, every [`asset_cache_dir`] resolves under it instead of the
/// platform cache root, so the asset stores serve from the replay bundle's
/// drop-in `cache/` (`<root>/<kind>/<first-char>/<uuid>.<ext>`) with no grid.
static REPLAY_CACHE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Point every [`asset_cache_dir`] at `root` for the rest of the process (the
/// replay bundle's `cache/`). Idempotent — only the first call takes effect.
pub(crate) fn set_replay_cache_root(root: PathBuf) {
    let _ignored = REPLAY_CACHE_ROOT.set(root);
}

/// Restart-scoped overrides resolved from the persisted settings store once,
/// pre-app, by [`crate::run_viewer`] (the [`REPLAY_CACHE_ROOT`] idiom): the
/// network & cache preferences tab's cache root / chat-log root / cache-size
/// settings are consumed at store-construction time, so they cannot be Bevy
/// resources — the stores are built before (or independent of) the app's
/// `ViewerSettings`.
#[derive(Debug, Clone, Default)]
pub(crate) struct StartupOverrides {
    /// A custom cache root replacing the platform cache directory (the
    /// `CacheLocation` setting; `None` = platform default).
    pub(crate) cache_root: Option<PathBuf>,
    /// A custom chat-log accounts root replacing the platform state directory
    /// (the `ChatLogLocation` setting; `None` = platform default).
    pub(crate) chat_log_base: Option<PathBuf>,
    /// The texture disk cache's size ceiling in bytes (`TextureCacheSizeMb`).
    pub(crate) texture_cache_max_bytes: Option<u64>,
    /// Each asset/mesh disk cache's size ceiling in bytes (`AssetCacheSizeMb`).
    pub(crate) asset_cache_max_bytes: Option<u64>,
}

/// The process-global [`StartupOverrides`], set once by [`crate::run_viewer`]
/// before the app is built. Unset (replay mode, tests) everything falls back
/// to the platform defaults.
static STARTUP_OVERRIDES: OnceLock<StartupOverrides> = OnceLock::new();

/// Installs the restart-scoped overrides for the rest of the process.
/// Idempotent — only the first call takes effect.
pub(crate) fn set_startup_overrides(overrides: StartupOverrides) {
    let _ignored = STARTUP_OVERRIDES.set(overrides);
}

/// The default per-cache size ceiling when no override is set — 2 GiB, the
/// same value as the asset-store crates' `CacheLimits::default()`, so an
/// unset override changes nothing.
const DEFAULT_CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The texture disk cache's size ceiling in bytes (the `TextureCacheSizeMb`
/// setting, or the 2 GiB default when no override was installed).
pub(crate) fn texture_cache_max_bytes() -> u64 {
    STARTUP_OVERRIDES
        .get()
        .and_then(|overrides| overrides.texture_cache_max_bytes)
        .unwrap_or(DEFAULT_CACHE_MAX_BYTES)
}

/// The per-asset-cache size ceiling in bytes, applied to each of the
/// mesh / material / bake-input / animation / environment / sound stores
/// independently (the `AssetCacheSizeMb` setting, or the 2 GiB default when
/// no override was installed).
pub(crate) fn asset_cache_max_bytes() -> u64 {
    STARTUP_OVERRIDES
        .get()
        .and_then(|overrides| overrides.asset_cache_max_bytes)
        .unwrap_or(DEFAULT_CACHE_MAX_BYTES)
}

/// The filename of the global settings file within the config root.
const GLOBAL_SETTINGS_FILE: &str = "viewer-settings.toml";

/// The subdirectory of the data root holding the per-avatar account directories.
const ACCOUNTS_SUBDIR: &str = "accounts";

/// The viewer's platform directories, or `None` when the platform has no home
/// directory (the caller then falls back to an in-memory or working-directory
/// path).
fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("net", "taladar", "sl-client-bevy-viewer")
}

/// A named content-addressed asset cache directory under the cache root (e.g.
/// `texturecache`, `meshcache`), or `None` when the platform has no cache
/// directory (the asset store then runs in-memory only).
pub(crate) fn asset_cache_dir(kind: &str) -> Option<PathBuf> {
    // In replay mode every cache resolves under the bundle's `cache/` instead of
    // the platform cache root, so the asset stores serve from the bundle.
    if let Some(root) = REPLAY_CACHE_ROOT.get() {
        return Some(root.join(kind));
    }
    Some(resolved_cache_root()?.join(kind))
}

/// The cache root every non-replay cache resolves under: the user's
/// `CacheLocation` override when one was installed, the platform cache
/// directory otherwise. `None` when neither exists.
pub(crate) fn resolved_cache_root() -> Option<PathBuf> {
    if let Some(root) = STARTUP_OVERRIDES
        .get()
        .and_then(|overrides| overrides.cache_root.clone())
    {
        return Some(root);
    }
    Some(project_dirs()?.cache_dir().to_path_buf())
}

/// The accounts root under the **config** directory, holding each avatar's
/// account-scope `settings.toml`, or `None` when the platform has no config
/// directory (per-avatar settings are then disabled).
pub(crate) fn config_accounts_base() -> Option<PathBuf> {
    Some(project_dirs()?.config_dir().join(ACCOUNTS_SUBDIR))
}

/// The accounts root under the **state** directory, holding each avatar's chat
/// transcripts, or `None` when the platform has no state (or data) directory
/// (per-avatar chat logging is then disabled). Falls back to the data directory
/// on platforms the `directories` crate reports no state directory for.
pub(crate) fn state_accounts_base() -> Option<PathBuf> {
    if let Some(base) = STARTUP_OVERRIDES
        .get()
        .and_then(|overrides| overrides.chat_log_base.clone())
    {
        return Some(base.join(ACCOUNTS_SUBDIR));
    }
    let dirs = project_dirs()?;
    let root = dirs.state_dir().unwrap_or_else(|| dirs.data_dir());
    Some(root.join(ACCOUNTS_SUBDIR))
}

/// The accounts root under the **cache** directory, holding each avatar's
/// regenerable inventory cache, or `None` when the platform has no cache
/// directory (the per-avatar inventory cache is then disabled).
pub(crate) fn cache_accounts_base() -> Option<PathBuf> {
    Some(resolved_cache_root()?.join(ACCOUNTS_SUBDIR))
}

/// The directory disk snapshots are written to
/// ([`crate::snapshot_floater`]), or `None` when the platform exposes no home
/// directory at all (the floater then disables the disk destination).
///
/// Unlike the settings / chat / cache trees this is **not** per-avatar and not
/// hidden under an XDG data root: a snapshot is a photo the user wants to find
/// and share, so it lands in the standard **Pictures** directory under a named
/// subfolder — the reference viewer's "Snapshots" folder convention. When the
/// platform reports no Pictures directory it falls back to the data root.
pub(crate) fn snapshots_dir() -> Option<PathBuf> {
    if let Some(dirs) = directories::UserDirs::new()
        && let Some(pictures) = dirs.picture_dir()
    {
        return Some(pictures.join(SNAPSHOTS_SUBDIR));
    }
    Some(project_dirs()?.data_dir().join("snapshots"))
}

/// The Pictures-directory subfolder disk snapshots land in.
const SNAPSHOTS_SUBDIR: &str = "sl-client-bevy-viewer snapshots";

/// The web-media (CEF) engine's cache root under the **cache** directory —
/// Chromium's disk caches and logs, shared across avatars like the asset
/// caches — or `None` when the platform has no cache directory (the engine
/// then keeps its caches under the working directory).
pub(crate) fn media_engine_cache_dir() -> Option<PathBuf> {
    Some(resolved_cache_root()?.join("cef"))
}

/// The machine-wide global settings file, under the config root — falling back
/// to the working directory when the platform has no config directory.
pub(crate) fn global_settings_file() -> PathBuf {
    project_dirs().map_or_else(
        || PathBuf::from(GLOBAL_SETTINGS_FILE),
        |dirs| dirs.config_dir().join(GLOBAL_SETTINGS_FILE),
    )
}

// ---------------------------------------------------------------------------
// Clear cache on next start.
// ---------------------------------------------------------------------------

/// The marker filename (in the resolved cache root) that requests a cache
/// purge on the next viewer start. A marker file rather than a settings flag
/// because the purge runs pre-app, where the async settings-save machinery
/// (Bevy's `IoTaskPool`) does not exist yet — and it lives beside what it
/// purges, so it stays correct under a custom cache root.
const PURGE_MARKER_FILE: &str = "clear-cache-on-next-run";

/// The cache-root subdirectories a "clear cache" purge deletes: every
/// regenerable content-addressed cache. Deliberately absent: `accounts` (the
/// per-avatar inventory cache has its own "clear inventory cache" action)
/// and `cef` (the embedded browser's cache is a future browser-cache task).
const PURGE_KIND_DIRS: &[&str] = &[
    "texturecache",
    "meshcache",
    "materialcache",
    "assetcache",
    "animcache",
    "envcache",
    "soundcache",
    "maptiles",
];

/// Requests a cache purge on the next viewer start by dropping the marker
/// file into the resolved cache root (creating the root if needed).
///
/// # Errors
///
/// Returns the I/O error if the root cannot be created or the marker not
/// written, or [`std::io::ErrorKind::NotFound`] when the platform has no
/// cache directory at all.
pub(crate) fn mark_cache_for_purge() -> std::io::Result<()> {
    let Some(root) = resolved_cache_root() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no cache directory on this platform",
        ));
    };
    fs_err::create_dir_all(&root)?;
    fs_err::write(root.join(PURGE_MARKER_FILE), b"")
}

/// Deletes the asset caches now if a previous session requested it (the
/// "clear cache" preferences action), then removes the marker. Runs pre-app
/// in [`crate::run_viewer`], before any store opened its directory.
pub(crate) fn purge_caches_if_marked() {
    let Some(root) = resolved_cache_root() else {
        return;
    };
    if !root.join(PURGE_MARKER_FILE).exists() {
        return;
    }
    purge_caches_in(&root);
}

/// The testable purge core: deletes every [`PURGE_KIND_DIRS`] subdirectory of
/// `root` (missing ones are fine, other failures are logged and skipped —
/// never fatal), then removes the marker file so the purge runs once.
fn purge_caches_in(root: &std::path::Path) {
    let mut purged = 0_u32;
    for kind in PURGE_KIND_DIRS {
        let dir = root.join(kind);
        if !dir.exists() {
            continue;
        }
        match fs_err::remove_dir_all(&dir) {
            Ok(()) => purged = purged.saturating_add(1),
            Err(error) => tracing::warn!("could not clear cache directory {dir:?}: {error}"),
        }
    }
    tracing::info!("cleared {purged} cache directories under {root:?} (marker present)");
    if let Err(error) = fs_err::remove_file(root.join(PURGE_MARKER_FILE)) {
        tracing::warn!("could not remove the cache-purge marker: {error}");
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{PURGE_MARKER_FILE, purge_caches_in};

    /// A unique throwaway directory under the system temp dir (the crate has
    /// no `tempfile` dependency; this mirrors sl-settings' test helper).
    fn tempdir() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after the epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "{}-paths-{nanos}-{:?}",
            env!("CARGO_PKG_NAME"),
            std::thread::current().id()
        ));
        fs_err::create_dir_all(&dir).expect("temp cache root");
        dir
    }

    #[test]
    fn purge_deletes_kind_dirs_and_marker_but_not_accounts() {
        let root = tempdir();
        // A populated cache tree: two kind dirs with content, the per-avatar
        // accounts tree, and the purge marker.
        for dir in ["texturecache/0", "meshcache/a", "accounts/grid/avatar"] {
            fs_err::create_dir_all(root.join(dir)).expect("seed dir");
        }
        fs_err::write(root.join("texturecache/0/x.asset"), b"x").expect("seed file");
        fs_err::write(root.join(PURGE_MARKER_FILE), b"").expect("seed marker");

        purge_caches_in(&root);

        // The caches and the marker are gone; the accounts tree survives.
        assert!(!root.join("texturecache").exists());
        assert!(!root.join("meshcache").exists());
        assert!(!root.join(PURGE_MARKER_FILE).exists());
        assert!(root.join("accounts/grid/avatar").exists());

        fs_err::remove_dir_all(&root).expect("cleanup");
    }
}
