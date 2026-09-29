//! The viewer's on-disk locations, resolved through the platform's standard
//! directories (`directories` crate: XDG on Linux, the equivalents elsewhere).
//!
//! Each kind of persistence lands under the XDG root that fits its category, so
//! a per-avatar `accounts/<grid>/<name>/` tree exists independently under three
//! roots (each keyed by grid + avatar name with UUID rename discovery — see
//! `sl_account_dirs`):
//!
//! - **config** (`~/.config/sl-client-bevy-viewer`) — the machine-wide
//!   `sl_settings::Scope::Global` settings file, and the per-avatar
//!   `sl_settings::Scope::Account` settings under
//!   [`ViewerPaths::config_accounts_base`].
//! - **state** (`~/.local/state/sl-client-bevy-viewer`) — the per-avatar chat
//!   transcripts under [`ViewerPaths::state_accounts_base`] (user-facing log state).
//! - **cache** (`~/.cache/sl-client-bevy-viewer`) — the content-addressed asset
//!   caches (textures / meshes / materials / animations / bake inputs), keyed by
//!   asset UUID and shared across every avatar and grid; plus the per-avatar,
//!   regenerable inventory cache under [`ViewerPaths::cache_accounts_base`].
//!
//! The cache root matches the location the asset caches used before this module
//! (`$XDG_CACHE_HOME`/`~/.cache` + `sl-client-bevy-viewer`), so moving them onto
//! the `directories` crate does not invalidate an existing cache.
//!
//! # Per viewer, not per process
//!
//! Every location is read from a [`ViewerPaths`] resource, which the viewer's
//! assembly inserts before any store is built — never from a process-wide
//! global — so two viewers in one process (the automation tier's in-process
//! backend) keep their settings, chat logs and caches apart. A world **without**
//! one keeps nothing on disk: a unit test's texture store runs in memory rather
//! than in the developer's cache.
//!
//! One thing here is deliberately process-wide: the web-media profile
//! ([`claim_media_engine_profile`]), because Chromium initialises once per
//! process and its profile with it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::prelude::{Resource, World};
use directories::ProjectDirs;

/// The restart-scoped overrides the network & cache preferences tab stores,
/// resolved from the persisted settings once, pre-app, by `run_viewer`: the
/// cache root / chat-log root / cache-size settings are consumed at
/// store-construction time, so they are part of [`ViewerPaths`] rather than
/// live settings.
#[derive(Debug, Clone, Default)]
pub struct StartupOverrides {
    /// A custom cache root replacing the platform cache directory (the
    /// `CacheLocation` setting; `None` = platform default).
    pub cache_root: Option<PathBuf>,
    /// A custom chat-log accounts root replacing the platform state directory
    /// (the `ChatLogLocation` setting; `None` = platform default).
    pub chat_log_base: Option<PathBuf>,
    /// The texture disk cache's size ceiling in bytes (`TextureCacheSizeMb`).
    pub texture_cache_max_bytes: Option<u64>,
    /// Each asset/mesh disk cache's size ceiling in bytes (`AssetCacheSizeMb`).
    pub asset_cache_max_bytes: Option<u64>,
}

/// The default per-cache size ceiling when no override is set — 2 GiB, the
/// same value as the asset-store crates' `CacheLimits::default()`, so an
/// unset override changes nothing.
const DEFAULT_CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The filename of the global settings file within the config root.
const GLOBAL_SETTINGS_FILE: &str = "viewer-settings.toml";

/// The subdirectory of the data root holding the per-avatar account directories.
const ACCOUNTS_SUBDIR: &str = "accounts";

/// The Pictures-directory subfolder disk snapshots land in.
const SNAPSHOTS_SUBDIR: &str = "sl-client-bevy-viewer snapshots";

/// The viewer's platform directories, or `None` when the platform has no home
/// directory.
fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("net", "taladar", "sl-client-bevy-viewer")
}

/// Where one content-addressed asset store keeps its disk cache, and how large
/// it may grow: what a store's constructor needs of [`ViewerPaths`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskCache {
    /// The cache directory, or `None` for an in-memory-only store.
    pub dir: Option<PathBuf>,
    /// The disk cache's size ceiling in bytes.
    pub max_bytes: u64,
}

/// The directories one viewer keeps its state under.
#[derive(Debug, Clone)]
struct Roots {
    /// The config root: the global settings file and the per-avatar
    /// account-scope settings.
    config: PathBuf,
    /// The state root: the per-avatar chat transcripts.
    state: PathBuf,
    /// The cache root: the content-addressed asset caches and the per-avatar
    /// inventory caches.
    cache: PathBuf,
    /// Where disk snapshots and panoramas are written.
    snapshots: PathBuf,
}

/// Where **one viewer** keeps what it stores between sessions — see the
/// [module documentation](self). A Bevy resource: the viewer's assembly
/// inserts it, and every store reads its directory from it
/// ([`ViewerPaths::of`]).
///
/// `Default` is [`ViewerPaths::none`].
#[derive(Resource, Debug, Clone, Default)]
pub struct ViewerPaths {
    /// The directories, or `None` for a viewer that keeps nothing on disk.
    roots: Option<Roots>,
    /// The user's restart-scoped location and size overrides.
    overrides: StartupOverrides,
    /// The avatar-state **replay** bundle's drop-in cache: when set, every
    /// [`asset_cache_dir`](Self::asset_cache_dir) resolves under it, so the
    /// asset stores serve from the bundle
    /// (`<root>/<kind>/<first-char>/<uuid>.<ext>`) with no grid.
    replay_cache_root: Option<PathBuf>,
}

impl ViewerPaths {
    /// The user's own directories: the platform's standard ones (XDG on
    /// Linux), each under the `sl-client-bevy-viewer` project name. Nothing on
    /// disk when the platform has no home directory.
    #[must_use]
    pub fn platform() -> Self {
        let roots = project_dirs().map(|dirs| {
            let data = dirs.data_dir().to_path_buf();
            Roots {
                config: dirs.config_dir().to_path_buf(),
                state: dirs
                    .state_dir()
                    .map_or_else(|| data.clone(), Path::to_path_buf),
                cache: dirs.cache_dir().to_path_buf(),
                // A snapshot is a photo the user wants to find and share, so it
                // lands in the standard Pictures directory under a named
                // subfolder — the reference viewer's "Snapshots" folder
                // convention — or under the data root without one.
                snapshots: directories::UserDirs::new()
                    .and_then(|user| user.picture_dir().map(|dir| dir.join(SNAPSHOTS_SUBDIR)))
                    .unwrap_or_else(|| data.join("snapshots")),
            }
        });
        Self {
            roots,
            ..Self::default()
        }
    }

    /// Every directory under one `root` (`config/`, `state/`, `cache/` and
    /// `snapshots/`): a test viewer's own tree, which neither reads the
    /// developer's preferences nor leaves anything outside `root`.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self {
            roots: Some(Roots {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
                snapshots: root.join("snapshots"),
            }),
            ..Self::default()
        }
    }

    /// Nothing on disk: no settings file, no chat log, no inventory cache, and
    /// every asset store in memory only.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// These paths with the user's restart-scoped overrides applied.
    #[must_use]
    pub fn with_startup_overrides(self, overrides: StartupOverrides) -> Self {
        Self { overrides, ..self }
    }

    /// These paths with every asset cache served from a replay bundle's
    /// drop-in `cache/` directory.
    #[must_use]
    pub fn with_replay_cache_root(self, root: PathBuf) -> Self {
        Self {
            replay_cache_root: Some(root),
            ..self
        }
    }

    /// The paths `world` keeps its state under: its [`ViewerPaths`] resource,
    /// or [`none`](Self::none) when it has none.
    #[must_use]
    pub fn of(world: &World) -> Self {
        world.get_resource::<Self>().cloned().unwrap_or_default()
    }

    /// Whether this viewer keeps anything on disk at all.
    #[must_use]
    pub const fn stores_anything(&self) -> bool {
        self.roots.is_some()
    }

    /// The texture disk cache's size ceiling in bytes (the `TextureCacheSizeMb`
    /// setting, or the 2 GiB default).
    #[must_use]
    pub fn texture_cache_max_bytes(&self) -> u64 {
        self.overrides
            .texture_cache_max_bytes
            .unwrap_or(DEFAULT_CACHE_MAX_BYTES)
    }

    /// The per-asset-cache size ceiling in bytes, applied to each of the
    /// mesh / material / bake-input / animation / environment / sound stores
    /// independently (the `AssetCacheSizeMb` setting, or the 2 GiB default).
    #[must_use]
    pub fn asset_cache_max_bytes(&self) -> u64 {
        self.overrides
            .asset_cache_max_bytes
            .unwrap_or(DEFAULT_CACHE_MAX_BYTES)
    }

    /// The disk cache of the asset store `kind` (e.g. `meshcache`): its
    /// [`asset_cache_dir`](Self::asset_cache_dir) and the per-asset-cache
    /// ceiling.
    #[must_use]
    pub fn asset_cache(&self, kind: &str) -> DiskCache {
        DiskCache {
            dir: self.asset_cache_dir(kind),
            max_bytes: self.asset_cache_max_bytes(),
        }
    }

    /// The texture store's disk cache, which has a ceiling of its own.
    #[must_use]
    pub fn texture_cache(&self) -> DiskCache {
        DiskCache {
            dir: self.asset_cache_dir("texturecache"),
            max_bytes: self.texture_cache_max_bytes(),
        }
    }

    /// A named content-addressed asset cache directory under the cache root
    /// (e.g. `texturecache`, `meshcache`), or `None` when this viewer has no
    /// cache directory (the asset store then runs in-memory only).
    #[must_use]
    pub fn asset_cache_dir(&self, kind: &str) -> Option<PathBuf> {
        // In replay mode every cache resolves under the bundle's `cache/`
        // instead of the cache root, so the asset stores serve from the bundle.
        if let Some(root) = &self.replay_cache_root {
            return Some(root.join(kind));
        }
        Some(self.resolved_cache_root()?.join(kind))
    }

    /// The **live** cache directory for `kind`, ignoring a replay bundle: where
    /// a capture copies assets from.
    #[must_use]
    pub fn live_asset_cache_dir(&self, kind: &str) -> Option<PathBuf> {
        Some(self.resolved_cache_root()?.join(kind))
    }

    /// The cache root every non-replay cache resolves under: the user's
    /// `CacheLocation` override when one was set, the cache directory
    /// otherwise. `None` when this viewer keeps nothing on disk.
    #[must_use]
    pub fn resolved_cache_root(&self) -> Option<PathBuf> {
        let roots = self.roots.as_ref()?;
        Some(
            self.overrides
                .cache_root
                .clone()
                .unwrap_or_else(|| roots.cache.clone()),
        )
    }

    /// The accounts root under the **config** directory, holding each
    /// avatar's account-scope `settings.toml`, or `None` when there is none
    /// (per-avatar settings are then disabled).
    #[must_use]
    pub fn config_accounts_base(&self) -> Option<PathBuf> {
        Some(self.roots.as_ref()?.config.join(ACCOUNTS_SUBDIR))
    }

    /// The accounts root under the **state** directory, holding each avatar's
    /// chat transcripts (or under the user's `ChatLogLocation`), or `None`
    /// when there is none (per-avatar chat logging is then disabled).
    #[must_use]
    pub fn state_accounts_base(&self) -> Option<PathBuf> {
        let roots = self.roots.as_ref()?;
        Some(
            self.overrides
                .chat_log_base
                .as_ref()
                .unwrap_or(&roots.state)
                .join(ACCOUNTS_SUBDIR),
        )
    }

    /// The accounts root under the **cache** directory, holding each avatar's
    /// regenerable inventory cache, or `None` when there is none (the
    /// per-avatar inventory cache is then disabled).
    #[must_use]
    pub fn cache_accounts_base(&self) -> Option<PathBuf> {
        Some(self.resolved_cache_root()?.join(ACCOUNTS_SUBDIR))
    }

    /// The directory disk snapshots and panoramas are written to, or `None`
    /// when this viewer keeps nothing on disk (the floater then disables the
    /// disk destination).
    #[must_use]
    pub fn snapshots_dir(&self) -> Option<PathBuf> {
        Some(self.roots.as_ref()?.snapshots.clone())
    }

    /// The machine-wide global settings file under the config root, or `None`
    /// when this viewer keeps nothing on disk (every setting then reads its
    /// declared default and nothing is written).
    #[must_use]
    pub fn global_settings_file(&self) -> Option<PathBuf> {
        Some(self.roots.as_ref()?.config.join(GLOBAL_SETTINGS_FILE))
    }

    /// The web-media (CEF) engine's directory under the **cache** root: the
    /// parent of the per-process profile directories
    /// [`claim_media_engine_profile`] hands out, or `None` when there is no
    /// cache directory.
    #[must_use]
    pub fn media_engine_cache_dir(&self) -> Option<PathBuf> {
        Some(self.resolved_cache_root()?.join("cef"))
    }

    /// Requests a cache purge on the next viewer start by dropping the marker
    /// file into the resolved cache root (creating the root if needed).
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the root cannot be created or the marker not
    /// written, or [`std::io::ErrorKind::NotFound`] when this viewer has no
    /// cache directory at all.
    pub fn mark_cache_for_purge(&self) -> std::io::Result<()> {
        let Some(root) = self.resolved_cache_root() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "this viewer has no cache directory",
            ));
        };
        fs_err::create_dir_all(&root)?;
        fs_err::write(root.join(PURGE_MARKER_FILE), b"")
    }

    /// Deletes the asset caches now if a previous session requested it (the
    /// "clear cache" preferences action), then removes the marker. Runs
    /// pre-app in `run_viewer`, before any store opened its directory.
    pub fn purge_caches_if_marked(&self) {
        let Some(root) = self.resolved_cache_root() else {
            return;
        };
        if !root.join(PURGE_MARKER_FILE).exists() {
            return;
        }
        purge_caches_in(&root);
    }
}

/// How many profile directories [`claim_media_engine_profile`] tries — far more
/// viewers and galleries than anyone runs side by side.
const MEDIA_ENGINE_PROFILE_SLOTS: u32 = 64;

/// The file inside a profile directory whose exclusive lock marks the
/// directory as this process's.
const MEDIA_ENGINE_PROFILE_LOCK: &str = "sl-viewer-profile.lock";

/// The claimed profile's lock file, held open (and so locked) for the rest of
/// the process. The operating system drops the lock when the process ends, a
/// crash included, which frees the directory for the next viewer.
///
/// **Process-wide on purpose**: Chromium initialises once per process, with
/// one profile, however many viewer Apps the process runs.
static MEDIA_ENGINE_PROFILE: OnceLock<(PathBuf, fs_err::File)> = OnceLock::new();

/// Where the web-media profiles live for a viewer with no cache directory,
/// under the system temp directory: a viewer that stores nothing must not
/// leave a profile in its working directory.
const MEDIA_ENGINE_FALLBACK_DIR: &str = "sl-viewer-cef-cache";

/// Why no web-media profile directory could be claimed.
#[derive(Debug, thiserror::Error)]
pub enum MediaProfileError {
    /// Every one of the profile directories is held by another running viewer.
    #[error("all {slots} web-media profile directories under {root} are held by other viewers")]
    AllInUse {
        /// The directory the profiles live under.
        root: String,
        /// How many there are.
        slots: u32,
    },
    /// A profile directory or its lock file could not be created or locked.
    #[error("web-media profile directory: {0}")]
    Io(#[from] std::io::Error),
}

/// This process's own web-media (CEF) profile directory — Chromium's cache,
/// cookies, local storage and logs — claimed on first call under `root` (a
/// viewer's [`ViewerPaths::media_engine_cache_dir`]) and the same for the rest
/// of the process, whatever a later caller passes: Chromium starts once per
/// process, so the first viewer App to start it decides where.
///
/// Chromium takes an exclusive lock on its profile, so two viewers (or a viewer
/// and the gallery) given one directory cannot both start it: the second one's
/// `cef::initialize` fails and it runs without web media. So each process
/// claims its own: the first of `cef/profile-0`, `cef/profile-1`, … whose lock
/// file no running process holds. A numbered slot rather than one per process
/// id, because a slot is **reused** — a lone viewer comes back to the same
/// profile every run, keeping a page's cookies and logins, and directories do
/// not pile up for every process that ever ran.
///
/// # Errors
///
/// [`MediaProfileError`] when every slot is held, or a directory cannot be
/// created or locked. (With no `root` the profiles live under the system temp
/// directory.)
pub fn claim_media_engine_profile(root: Option<&Path>) -> Result<PathBuf, MediaProfileError> {
    if let Some((dir, _lock)) = MEDIA_ENGINE_PROFILE.get() {
        return Ok(dir.clone());
    }
    let root = root.map_or_else(
        || std::env::temp_dir().join(MEDIA_ENGINE_FALLBACK_DIR),
        Path::to_path_buf,
    );
    let (dir, lock) = claim_profile_slot(&root, MEDIA_ENGINE_PROFILE_SLOTS)?;
    // A second claim racing this one within the process keeps the first; its
    // own lock file closes (and unlocks) as it drops.
    let (claimed, _lock) = MEDIA_ENGINE_PROFILE.get_or_init(|| (dir, lock));
    Ok(claimed.clone())
}

/// Claim the first of `root/profile-0` … `root/profile-{slots - 1}` whose lock
/// file no one else holds, creating it as needed; returns the directory and the
/// locked file, which keeps the claim for as long as it stays open.
fn claim_profile_slot(
    root: &std::path::Path,
    slots: u32,
) -> Result<(PathBuf, fs_err::File), MediaProfileError> {
    for slot in 0..slots {
        let dir = root.join(format!("profile-{slot}"));
        fs_err::create_dir_all(&dir)?;
        let lock = fs_err::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(MEDIA_ENGINE_PROFILE_LOCK))?;
        match lock.try_lock() {
            Ok(()) => return Ok((dir, lock)),
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    Err(MediaProfileError::AllInUse {
        root: root.display().to_string(),
        slots,
    })
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

    use pretty_assertions::{assert_eq, assert_ne};

    use super::{
        MediaProfileError, PURGE_MARKER_FILE, StartupOverrides, ViewerPaths, claim_profile_slot,
        purge_caches_in,
    };
    use std::path::Path;

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

    /// Two claims each get a profile of their own while both hold theirs, a
    /// released one is reused, and a full set says so rather than sharing.
    #[test]
    fn each_claim_gets_its_own_profile_and_a_released_one_is_reused() {
        let root = tempdir();
        let (first, first_lock) = claim_profile_slot(&root, 2).expect("first claim");
        let (second, second_lock) = claim_profile_slot(&root, 2).expect("second claim");
        assert_eq!(first, root.join("profile-0"));
        assert_eq!(second, root.join("profile-1"));
        assert!(matches!(
            claim_profile_slot(&root, 2),
            Err(MediaProfileError::AllInUse { slots: 2, .. })
        ));

        drop(first_lock);
        let (again, _again_lock) = claim_profile_slot(&root, 2).expect("reclaim");
        assert_eq!(
            again, first,
            "a released profile comes back to the next viewer"
        );

        drop(second_lock);
        fs_err::remove_dir_all(&root).expect("cleanup");
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

    /// Two viewers rooted apart share no directory: not the settings file,
    /// not a cache, not the chat logs.
    #[test]
    fn two_viewers_rooted_apart_share_nothing() {
        let first = ViewerPaths::under(Path::new("/tmp/first"));
        let second = ViewerPaths::under(Path::new("/tmp/second"));
        assert_eq!(
            first.global_settings_file(),
            Some(Path::new("/tmp/first/config/viewer-settings.toml").to_path_buf())
        );
        assert_ne!(first.global_settings_file(), second.global_settings_file());
        assert_ne!(
            first.asset_cache_dir("texturecache"),
            second.asset_cache_dir("texturecache")
        );
        assert_ne!(first.state_accounts_base(), second.state_accounts_base());
        assert_ne!(first.snapshots_dir(), second.snapshots_dir());
    }

    /// A viewer with no paths keeps nothing anywhere, and a world with no
    /// paths resource is such a viewer.
    #[test]
    fn no_paths_store_nothing() {
        let none = ViewerPaths::of(&bevy::prelude::World::new());
        assert!(!none.stores_anything());
        assert_eq!(none.global_settings_file(), None);
        assert_eq!(none.asset_cache_dir("texturecache"), None);
        assert_eq!(none.config_accounts_base(), None);
        assert_eq!(none.state_accounts_base(), None);
        assert_eq!(none.cache_accounts_base(), None);
        assert_eq!(none.snapshots_dir(), None);
        assert!(none.mark_cache_for_purge().is_err());
    }

    /// The user's overrides move the cache and the chat logs and set the
    /// ceilings; the replay bundle wins over both for the asset caches only.
    #[test]
    fn overrides_and_the_replay_root_apply_per_viewer() {
        let paths =
            ViewerPaths::under(Path::new("/tmp/viewer")).with_startup_overrides(StartupOverrides {
                cache_root: Some("/tmp/elsewhere".into()),
                chat_log_base: Some("/tmp/logs".into()),
                texture_cache_max_bytes: Some(7),
                asset_cache_max_bytes: None,
            });
        assert_eq!(
            paths.asset_cache_dir("meshcache"),
            Some(Path::new("/tmp/elsewhere/meshcache").to_path_buf())
        );
        assert_eq!(
            paths.state_accounts_base(),
            Some(Path::new("/tmp/logs/accounts").to_path_buf())
        );
        assert_eq!(paths.texture_cache_max_bytes(), 7);
        assert_eq!(
            paths.asset_cache_max_bytes(),
            super::DEFAULT_CACHE_MAX_BYTES
        );
        let replay = paths.with_replay_cache_root("/tmp/bundle/cache".into());
        assert_eq!(
            replay.asset_cache_dir("meshcache"),
            Some(Path::new("/tmp/bundle/cache/meshcache").to_path_buf())
        );
        assert_eq!(
            replay.live_asset_cache_dir("meshcache"),
            Some(Path::new("/tmp/elsewhere/meshcache").to_path_buf())
        );
        assert_eq!(
            ViewerPaths::default().asset_cache_max_bytes(),
            super::DEFAULT_CACHE_MAX_BYTES
        );
    }
}
