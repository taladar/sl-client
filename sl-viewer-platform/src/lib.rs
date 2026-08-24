//! The viewer's platform layer: where it puts things on this machine, and the
//! small host-facing helpers that have no opinion about the world being
//! rendered.
//!
//! Everything here is a leaf. Nothing in this crate reaches back into the
//! viewer, which is what lets it compile alongside the rest of the workspace
//! rather than behind it, and what keeps an edit here from rebuilding anything
//! but its own dependents.
//!
//! - [`paths`] — the XDG directory layout: settings, cache, chat logs,
//!   snapshots. The one module whose constants are load-bearing for existing
//!   installs, since they name the directories a user's data already lives in.
//! - [`sound_cache`] and [`environment_assets`] — on-disk caches keyed through
//!   [`paths`].
//! - [`asset_retry`] — the backoff policy for a fetch that failed.
//! - [`clipboard`] — OS clipboard access.
//! - [`ui_perf`] — UI frame-timing counters.
//! - [`url_linkify`] — finding URLs, SLURLs and avatar/group references in
//!   arbitrary text so chat can render them as links.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its Bevy plugin \
              and systems read as `sound_cache::SoundCachePlugin` / \
              `sound_cache::poll_sound_cache`. That only became a lint when these \
              items turned `pub` for the crate split; renaming them would churn \
              every call site in the app builder to satisfy a style rule this \
              codebase does not follow"
)]

pub mod asset_retry;
pub mod clipboard;
pub mod environment_assets;
pub mod paths;
pub mod sound_cache;
pub mod system_browser;
pub mod ui_perf;
pub mod url_linkify;
