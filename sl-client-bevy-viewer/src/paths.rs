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

use directories::ProjectDirs;

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
    Some(project_dirs()?.cache_dir().join(kind))
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
    let dirs = project_dirs()?;
    let root = dirs.state_dir().unwrap_or_else(|| dirs.data_dir());
    Some(root.join(ACCOUNTS_SUBDIR))
}

/// The accounts root under the **cache** directory, holding each avatar's
/// regenerable inventory cache, or `None` when the platform has no cache
/// directory (the per-avatar inventory cache is then disabled).
pub(crate) fn cache_accounts_base() -> Option<PathBuf> {
    Some(project_dirs()?.cache_dir().join(ACCOUNTS_SUBDIR))
}

/// The web-media (CEF) engine's cache root under the **cache** directory —
/// Chromium's disk caches and logs, shared across avatars like the asset
/// caches — or `None` when the platform has no cache directory (the engine
/// then keeps its caches under the working directory).
pub(crate) fn media_engine_cache_dir() -> Option<PathBuf> {
    Some(project_dirs()?.cache_dir().join("cef"))
}

/// The machine-wide global settings file, under the config root — falling back
/// to the working directory when the platform has no config directory.
pub(crate) fn global_settings_file() -> PathBuf {
    project_dirs().map_or_else(
        || PathBuf::from(GLOBAL_SETTINGS_FILE),
        |dirs| dirs.config_dir().join(GLOBAL_SETTINGS_FILE),
    )
}
