//! The viewer's on-disk locations, resolved through the platform's standard
//! directories (`directories` crate: XDG on Linux, the equivalents elsewhere).
//!
//! Three roots are used:
//!
//! - **config** (`~/.config/sl-client-bevy-viewer`) — the machine-wide
//!   [`Global`](sl_settings::Scope::Global) settings file.
//! - **cache** (`~/.cache/sl-client-bevy-viewer`) — the content-addressed asset
//!   caches (textures / meshes / materials / animations / bake inputs), keyed by
//!   asset UUID and shared across every avatar and grid.
//! - **data** (`~/.local/share/sl-client-bevy-viewer`) — the per-avatar
//!   directories under [`accounts_base`], holding one avatar's account settings,
//!   chat transcripts and inventory cache, keyed by grid + avatar name (see
//!   [`sl_account_dirs`]).
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

/// The accounts root under the data directory, under which the per-avatar
/// `<grid>/<name>/` directories live, or `None` when the platform has no data
/// directory (per-avatar persistence is then disabled).
pub(crate) fn accounts_base() -> Option<PathBuf> {
    Some(project_dirs()?.data_dir().join(ACCOUNTS_SUBDIR))
}

/// The machine-wide global settings file, under the config root — falling back
/// to the working directory when the platform has no config directory.
pub(crate) fn global_settings_file() -> PathBuf {
    project_dirs().map_or_else(
        || PathBuf::from(GLOBAL_SETTINGS_FILE),
        |dirs| dirs.config_dir().join(GLOBAL_SETTINGS_FILE),
    )
}
