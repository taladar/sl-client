//! The Preferences **network & cache** tab
//! (`viewer-preferences-network-cache-tab`).
//!
//! The network / cache tab of the preferences floater
//! ([`crate::preferences`]):
//!
//! - **Maximum bandwidth** — a **live** control: `apply_throttle` folds the
//!   slider value through [`Throttle::from_total`] (the reference viewer's
//!   preset-interpolating split) into an `AgentThrottle`, re-sent on change
//!   and on every region handshake (replacing the previously hardcoded
//!   1000 kbps preset in [`crate::session`]).
//! - **HTTP proxy** — one `host:port` proxy for *all* our HTTP traffic. The
//!   whole stack is reqwest, so the reference's separate web / "other HTTP"
//!   proxy split has nothing to distinguish here; SOCKS for the UDP circuit
//!   is out of scope (`roadmap/deferred/viewer-socks5-udp-proxy.md`), and the
//!   embedded CEF browser keeps its own network stack. **Restart-scoped**:
//!   reqwest clients only take a proxy at build time, so
//!   `crate::run_viewer` reads the persisted values pre-app and installs
//!   them via [`sl_client_bevy::http_proxy::set_proxy`].
//! - **Cache sizes** — the texture cache's and the asset caches' size
//!   ceilings, in MB. The asset value governs **each** of the six parallel
//!   asset stores (mesh / material / bake-input / animation / environment /
//!   sound) independently — unlike the reference's single unified asset
//!   cache directory — with the 2048 MB default reproducing the previous
//!   fixed 2 GiB per store. Restart-scoped ([`crate::paths`] startup
//!   overrides).
//! - **Cache / chat-log locations** — free-text directory overrides (empty =
//!   the platform default), restart-scoped. The chat-log base is global
//!   where the reference's `InstantMessageLogPath` is per-account: the base
//!   is consumed before login, and the per-avatar `accounts/<grid>/<name>/`
//!   subtree under it preserves separation.
//! - **Clear cache** — confirmation first (the catalogue's
//!   `ConfirmClearCache`), then a purge **on the next start** (the stores
//!   hold their directories open for the whole session): the confirmation
//!   drops [`crate::paths::mark_cache_for_purge`]'s marker file, honoured by
//!   [`crate::paths::purge_caches_if_marked`] pre-app. **Clear inventory
//!   cache** deletes the per-avatar inventory snapshots immediately (they
//!   are only read at login and rewritten at logout).
//!
//! Reference (Firestorm, read-only): `panel_preferences_setup.xml`,
//! `floater_preferences_proxy.xml`, `llviewerthrottle.cpp`,
//! `llappviewer.cpp` (`initCache`).

use bevy::prelude::*;
use bevy::tasks::IoTaskPool;
use bevy::ui_widgets::{Activate, SliderRange, SliderStep};
use sl_client_bevy::{Command, Kilobits, SlCommand, SlEvent, SlSessionEvent, Throttle};
use sl_settings::{Scope, SettingValue};

use crate::notifications::{NotificationResponse, ShowNotification};
use crate::preferences::{
    spawn_pref_action, spawn_pref_checkbox, spawn_pref_section, spawn_pref_slider, spawn_pref_text,
};
use crate::preferences_camera_move::spawn_reset_button;
use crate::settings::ViewerSettings;
use crate::settings_binding::SettingBinding;
use crate::ui_text_input::TextInputKind;

/// The stable id of this tab in `crate::preferences::PREF_TABS`.
pub(crate) const TAB_ID: &str = "network-cache";

/// The settings section the network keys live in.
const NETWORK_SECTION: &[&str] = &["network"];

/// The settings section the cache keys live in.
const CACHE_SECTION: &[&str] = &["cache"];

/// The requested total UDP bandwidth, kbps (the reference
/// `ThrottleBandwidthKBPS`), split per category by [`Throttle::from_total`].
pub(crate) const SETTING_MAX_BANDWIDTH: &str = "ThrottleBandwidthKbps";

/// Whether the HTTP proxy is applied at the next start (the reference's
/// `BrowserProxyEnabled`, widened to all our HTTP traffic).
pub(crate) const SETTING_HTTP_PROXY_ENABLED: &str = "HttpProxyEnabled";

/// The HTTP proxy as `host:port` (the reference's `BrowserProxyAddress` +
/// `BrowserProxyPort` in one field — the text binding is string-typed).
pub(crate) const SETTING_HTTP_PROXY: &str = "HttpProxy";

/// The texture disk cache's size ceiling, MB (the reference `CacheSize`).
pub(crate) const SETTING_TEXTURE_CACHE_SIZE_MB: &str = "TextureCacheSizeMb";

/// Each asset disk cache's size ceiling, MB (the reference
/// `FSDiskCacheSize`; per-store here, see the module docs).
pub(crate) const SETTING_ASSET_CACHE_SIZE_MB: &str = "AssetCacheSizeMb";

/// A custom cache root directory; empty = the platform cache directory (the
/// reference `CacheLocationTopFolder`).
pub(crate) const SETTING_CACHE_LOCATION: &str = "CacheLocation";

/// A custom chat-log root directory; empty = the platform state directory
/// (the reference `InstantMessageLogPath`, global-scoped — module docs).
pub(crate) const SETTING_CHAT_LOG_LOCATION: &str = "ChatLogLocation";

/// The bandwidth default, kbps — the reference viewer's settings default.
/// This deliberately raises the previously hardcoded 1000 kbps preset.
const DEFAULT_MAX_BANDWIDTH_KBPS: f32 = 3000.0;

/// The cache-size default, MB — 2048 = the stores' previous fixed 2 GiB.
const DEFAULT_CACHE_SIZE_MB: u32 = 2048;

/// The cache-size sliders' bounds and step, MB (the reference setup panel).
const CACHE_SIZE_MIN_MB: f32 = 256.0;
/// See [`CACHE_SIZE_MIN_MB`].
const CACHE_SIZE_MAX_MB: f32 = 20000.0;
/// See [`CACHE_SIZE_MIN_MB`].
const CACHE_SIZE_STEP_MB: f32 = 64.0;

/// The bandwidth slider's bounds and step, kbps (the reference setup panel).
const BANDWIDTH_MIN_KBPS: f32 = 50.0;
/// See [`BANDWIDTH_MIN_KBPS`].
const BANDWIDTH_MAX_KBPS: f32 = 3000.0;
/// See [`BANDWIDTH_MIN_KBPS`].
const BANDWIDTH_STEP_KBPS: f32 = 50.0;

/// Register this tab's settings.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        NETWORK_SECTION,
        SETTING_MAX_BANDWIDTH,
        SettingValue::F32(DEFAULT_MAX_BANDWIDTH_KBPS),
        "Maximum UDP bandwidth requested from the simulator, kilobits per second",
    );
    settings.register_in(
        NETWORK_SECTION,
        SETTING_HTTP_PROXY_ENABLED,
        SettingValue::Bool(false),
        "Route all HTTP traffic through the HTTP proxy (takes effect on restart)",
    );
    settings.register_in(
        NETWORK_SECTION,
        SETTING_HTTP_PROXY,
        SettingValue::String(String::new()),
        "The HTTP proxy as host:port, e.g. 127.0.0.1:8888 (takes effect on restart)",
    );
    settings.register_in(
        CACHE_SECTION,
        SETTING_TEXTURE_CACHE_SIZE_MB,
        SettingValue::U32(DEFAULT_CACHE_SIZE_MB),
        "Texture disk cache size ceiling in MB (takes effect on restart)",
    );
    settings.register_in(
        CACHE_SECTION,
        SETTING_ASSET_CACHE_SIZE_MB,
        SettingValue::U32(DEFAULT_CACHE_SIZE_MB),
        "Size ceiling in MB for each asset disk cache (takes effect on restart)",
    );
    settings.register_in(
        CACHE_SECTION,
        SETTING_CACHE_LOCATION,
        SettingValue::String(String::new()),
        "Custom cache directory; empty = the platform default (takes effect on restart)",
    );
    settings.register_in(
        CACHE_SECTION,
        SETTING_CHAT_LOG_LOCATION,
        SettingValue::String(String::new()),
        "Custom chat-log directory; empty = the platform default (takes effect on restart)",
    );
}

/// Consume this tab's **restart-scoped** settings from the pre-app store
/// load in `crate::run_viewer`: install the cache / chat-log locations and
/// cache-size ceilings as [`crate::paths`] startup overrides, install the
/// HTTP proxy, and honour a pending clear-cache request — all before any
/// store or HTTP client is built.
pub fn apply_startup_settings(settings: &ViewerSettings) {
    let store = settings.store();
    crate::paths::set_startup_overrides(crate::paths::StartupOverrides {
        cache_root: validated_dir(store.get_str(SETTING_CACHE_LOCATION).ok(), "cache"),
        chat_log_base: validated_dir(store.get_str(SETTING_CHAT_LOG_LOCATION).ok(), "chat-log"),
        texture_cache_max_bytes: store
            .get_u32(SETTING_TEXTURE_CACHE_SIZE_MB)
            .ok()
            .map(megabytes_to_bytes),
        asset_cache_max_bytes: store
            .get_u32(SETTING_ASSET_CACHE_SIZE_MB)
            .ok()
            .map(megabytes_to_bytes),
    });
    let proxy_enabled = store.get_bool(SETTING_HTTP_PROXY_ENABLED).unwrap_or(false);
    let proxy = store.get_str(SETTING_HTTP_PROXY).unwrap_or_default();
    if proxy_enabled && !proxy.is_empty() {
        match sl_client_bevy::http_proxy::set_proxy(proxy) {
            Ok(()) => info!("routing all HTTP traffic through proxy {proxy}"),
            Err(error) => {
                warn!("invalid HTTP proxy setting {proxy:?} ({error}); connecting directly");
            }
        }
    }
    crate::paths::purge_caches_if_marked();
}

/// A settings value in MB as bytes (a u32 MB count times 2^20 always fits
/// a u64; saturating keeps the lint-visible arithmetic total).
fn megabytes_to_bytes(megabytes: u32) -> u64 {
    /// The bytes in one megabyte (2^20).
    const BYTES_PER_MEGABYTE: u64 = 0x0010_0000;
    u64::from(megabytes).saturating_mul(BYTES_PER_MEGABYTE)
}

/// A non-empty directory setting as a usable path: created if missing, and
/// dropped (falling back to the platform default) with a warning when it
/// cannot be — a location override must never silently scatter files.
fn validated_dir(setting: Option<&str>, what: &str) -> Option<std::path::PathBuf> {
    let value = setting?;
    if value.is_empty() {
        return None;
    }
    let path = std::path::PathBuf::from(value);
    match fs_err::create_dir_all(&path) {
        Ok(()) => Some(path),
        Err(error) => {
            warn!(
                "custom {what} location {path:?} is unusable ({error}); \
                 using the platform default"
            );
            None
        }
    }
}

/// Build the tab's content: the network, proxy and cache sections per the
/// module docs (all controls global scope — machine-wide, like the
/// reference's setup panel).
pub(crate) fn build_network_cache_tab(commands: &mut Commands, panel: Entity) {
    spawn_pref_section(commands, panel, "preferences-section-network");
    let bandwidth_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-max-bandwidth",
        SettingBinding::global(SETTING_MAX_BANDWIDTH),
        SliderRange::new(BANDWIDTH_MIN_KBPS, BANDWIDTH_MAX_KBPS),
        SliderStep(BANDWIDTH_STEP_KBPS),
    );
    spawn_reset_button(
        commands,
        bandwidth_row,
        Scope::Global,
        SETTING_MAX_BANDWIDTH,
    );

    spawn_pref_section(commands, panel, "preferences-section-proxy");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-http-proxy-enabled",
        SettingBinding::global(SETTING_HTTP_PROXY_ENABLED),
    );
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-http-proxy",
        SettingBinding::global(SETTING_HTTP_PROXY),
        TextInputKind::Line,
        1.0,
    );

    spawn_pref_section(commands, panel, "preferences-section-cache");
    let texture_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-texture-cache-size",
        SettingBinding::global(SETTING_TEXTURE_CACHE_SIZE_MB),
        SliderRange::new(CACHE_SIZE_MIN_MB, CACHE_SIZE_MAX_MB),
        SliderStep(CACHE_SIZE_STEP_MB),
    );
    spawn_reset_button(
        commands,
        texture_row,
        Scope::Global,
        SETTING_TEXTURE_CACHE_SIZE_MB,
    );
    let asset_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-asset-cache-size",
        SettingBinding::global(SETTING_ASSET_CACHE_SIZE_MB),
        SliderRange::new(CACHE_SIZE_MIN_MB, CACHE_SIZE_MAX_MB),
        SliderStep(CACHE_SIZE_STEP_MB),
    );
    spawn_reset_button(
        commands,
        asset_row,
        Scope::Global,
        SETTING_ASSET_CACHE_SIZE_MB,
    );
    let location_row = spawn_pref_text(
        commands,
        panel,
        "preferences-row-cache-location",
        SettingBinding::global(SETTING_CACHE_LOCATION),
        TextInputKind::Line,
        1.0,
    );
    spawn_reset_button(
        commands,
        location_row,
        Scope::Global,
        SETTING_CACHE_LOCATION,
    );
    let chat_log_row = spawn_pref_text(
        commands,
        panel,
        "preferences-row-chat-log-location",
        SettingBinding::global(SETTING_CHAT_LOG_LOCATION),
        TextInputKind::Line,
        1.0,
    );
    spawn_reset_button(
        commands,
        chat_log_row,
        Scope::Global,
        SETTING_CHAT_LOG_LOCATION,
    );
    let clear_cache = spawn_pref_action(
        commands,
        panel,
        "preferences-row-clear-cache",
        "preferences-clear-cache",
    );
    commands.entity(clear_cache).observe(
        |_activate: On<Activate>, mut show: MessageWriter<ShowNotification>| {
            show.write(ShowNotification::new("ConfirmClearCache"));
        },
    );
    let clear_inventory = spawn_pref_action(
        commands,
        panel,
        "preferences-row-clear-inventory-cache",
        "preferences-clear-inventory-cache",
    );
    commands.entity(clear_inventory).observe(
        |_activate: On<Activate>, mut show: MessageWriter<ShowNotification>| {
            show.write(ShowNotification::new("ConfirmClearInventoryCache"));
        },
    );
}

/// Announce the (user-tunable) bandwidth throttle to the simulator, re-sending
/// it whenever [`SETTING_MAX_BANDWIDTH`] changes and on every region handshake
/// (the [`crate::session::apply_draw_distance`] idiom). Without an
/// `AgentThrottle` the simulator streams objects at conservative defaults and
/// never reaches lower-priority interest-list entries (R22b), so this always
/// announces — the setting's default when untouched.
pub(crate) fn apply_throttle(
    settings: Option<Res<ViewerSettings>>,
    mut events: MessageReader<SlEvent>,
    mut applied: Local<Option<f32>>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(settings) = settings else {
        return;
    };
    // A fresh region must be told the throttle again (the session core also
    // re-advertises on region change, but a handshake after login is the
    // first send), so drop the memo on every handshake.
    if events
        .read()
        .any(|event| matches!(event.0, SlSessionEvent::RegionHandshakeComplete))
    {
        *applied = None;
    }
    let Ok(total) = settings.store().get_f32(SETTING_MAX_BANDWIDTH) else {
        return;
    };
    if *applied == Some(total) {
        return;
    }
    let rate = match Kilobits::new(total) {
        Ok(rate) => rate,
        Err(error) => {
            // Unreachable through the slider (bounded), only via a mangled
            // settings file; skip rather than advertise nonsense.
            warn!("ignoring invalid bandwidth setting {total}: {error}");
            return;
        }
    };
    *applied = Some(total);
    info!("announcing a {total} kbps bandwidth throttle to the simulator");
    commands.write(SlCommand(Command::SetThrottle(Throttle::from_total(rate))));
}

/// Answer the two clear-cache confirmations: **OK** on `ConfirmClearCache`
/// marks the asset caches for a purge on the next start
/// ([`crate::paths::mark_cache_for_purge`]); **OK** on
/// `ConfirmClearInventoryCache` deletes the per-avatar inventory snapshots
/// now, detached on the [`IoTaskPool`] (they are only read at login and
/// rewritten at logout, so a live session never re-reads them). Cancel and
/// dismissal do nothing.
fn handle_cache_clear_confirmations(mut responses: MessageReader<NotificationResponse>) {
    for response in responses.read() {
        if response.button != Some("OK") {
            continue;
        }
        match response.template {
            "ConfirmClearCache" => {
                if let Err(error) = crate::paths::mark_cache_for_purge() {
                    warn!("could not mark the cache for a purge on next start: {error}");
                } else {
                    info!("cache marked for a purge on the next start");
                }
            }
            "ConfirmClearInventoryCache" => {
                let Some(base) = crate::paths::cache_accounts_base() else {
                    continue;
                };
                IoTaskPool::get()
                    .spawn(async move { delete_inventory_caches(&base) })
                    .detach();
            }
            _ => {}
        }
    }
}

/// The suffix every per-avatar inventory snapshot filename ends in (the
/// library snapshot's `.lib.inv.llsd.gz` ends in it too).
const INVENTORY_CACHE_SUFFIX: &str = ".inv.llsd.gz";

/// Delete every inventory snapshot under the accounts tree
/// (`<base>/<grid>/<avatar>/<agent>[.lib].inv.llsd.gz`), logging (never
/// hiding) failures. Non-snapshot files and the directory structure itself
/// (including the UUID reverse-symlinks) are left alone.
fn delete_inventory_caches(base: &std::path::Path) {
    let Ok(grids) = fs_err::read_dir(base) else {
        // No accounts tree yet — nothing cached, nothing to do.
        return;
    };
    let mut deleted = 0_u32;
    for grid in grids.flatten() {
        let Ok(avatars) = fs_err::read_dir(grid.path()) else {
            continue;
        };
        for avatar in avatars.flatten() {
            let Ok(files) = fs_err::read_dir(avatar.path()) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                let is_snapshot = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(INVENTORY_CACHE_SUFFIX));
                if !is_snapshot {
                    continue;
                }
                match fs_err::remove_file(&path) {
                    Ok(()) => deleted = deleted.saturating_add(1),
                    // A second traversal through a UUID symlink can race the
                    // first deletion; gone is the goal either way.
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        tracing::warn!("could not delete inventory cache {path:?}: {error}");
                    }
                }
            }
        }
    }
    tracing::info!("deleted {deleted} inventory cache snapshots");
}

/// Owns the network & cache tab's runtime side: the live throttle applier and
/// the clear-cache confirmation routing. The tab *content* is built by the
/// preferences shell through `crate::preferences::PREF_TABS`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreferencesNetworkCachePlugin;

impl Plugin for PreferencesNetworkCachePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (apply_throttle, handle_cache_clear_confirmations));
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Command, Kilobits, SlCommand, SlEvent, SlSessionEvent, Throttle};
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        DEFAULT_CACHE_SIZE_MB, DEFAULT_MAX_BANDWIDTH_KBPS, SETTING_ASSET_CACHE_SIZE_MB,
        SETTING_CACHE_LOCATION, SETTING_CHAT_LOG_LOCATION, SETTING_HTTP_PROXY,
        SETTING_HTTP_PROXY_ENABLED, SETTING_MAX_BANDWIDTH, SETTING_TEXTURE_CACHE_SIZE_MB,
        apply_throttle, register_settings,
    };
    use crate::settings::ViewerSettings;

    /// A minimal app with the registered settings and the throttle applier.
    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        app.insert_resource(settings)
            .add_message::<SlEvent>()
            .add_message::<SlCommand>()
            .add_systems(Update, apply_throttle);
        app
    }

    /// The `SetThrottle` commands written since the last call.
    fn drain_throttles(app: &mut App) -> Vec<Throttle> {
        let mut commands = app.world_mut().resource_mut::<Messages<SlCommand>>();
        commands
            .drain()
            .filter_map(|command| match command.0 {
                Command::SetThrottle(throttle) => Some(throttle),
                _ => None,
            })
            .collect()
    }

    /// A freshly registered store pins the intended defaults: bandwidth 3000
    /// (the reference settings default — a deliberate raise over the
    /// previously hardcoded 1000), 2048 MB per cache, empty locations, proxy
    /// off and empty.
    #[test]
    fn registered_defaults_pin_the_intended_values() {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        let store = settings.store();
        assert_eq!(
            store.get_f32(SETTING_MAX_BANDWIDTH).ok(),
            Some(DEFAULT_MAX_BANDWIDTH_KBPS)
        );
        assert_eq!(
            store.get_u32(SETTING_TEXTURE_CACHE_SIZE_MB).ok(),
            Some(DEFAULT_CACHE_SIZE_MB)
        );
        assert_eq!(
            store.get_u32(SETTING_ASSET_CACHE_SIZE_MB).ok(),
            Some(DEFAULT_CACHE_SIZE_MB)
        );
        assert_eq!(store.get_bool(SETTING_HTTP_PROXY_ENABLED).ok(), Some(false));
        assert_eq!(store.get_str(SETTING_HTTP_PROXY).ok(), Some(""));
        assert_eq!(store.get_str(SETTING_CACHE_LOCATION).ok(), Some(""));
        assert_eq!(store.get_str(SETTING_CHAT_LOG_LOCATION).ok(), Some(""));
    }

    /// The throttle is announced once per handshake and once per setting
    /// change — never re-sent while nothing changed.
    #[test]
    fn throttle_sent_on_handshake_and_change_only() {
        let mut app = test_app();
        // Before any handshake: the default is announced once (a fresh applier
        // memo), then goes quiet.
        app.update();
        let initial = drain_throttles(&mut app);
        assert_eq!(
            initial,
            vec![Throttle::from_total(Kilobits::new_unchecked(
                DEFAULT_MAX_BANDWIDTH_KBPS
            ))]
        );
        app.update();
        assert_eq!(drain_throttles(&mut app), vec![], "unchanged → no re-send");
        // A handshake re-announces the same value (a fresh region must hear it).
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::RegionHandshakeComplete));
        app.update();
        assert_eq!(
            drain_throttles(&mut app).len(),
            1,
            "handshake → one re-send"
        );
        // A changed setting re-announces the new split.
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_MAX_BANDWIDTH,
            SettingValue::F32(500.0),
        );
        app.update();
        assert_eq!(
            drain_throttles(&mut app),
            vec![Throttle::preset_500()],
            "changed setting → the 500 kbps split"
        );
    }

    /// Building the tab into an empty panel spawns every searchable row — one
    /// network, two proxy, six cache — without panicking.
    #[test]
    fn build_spawns_every_row() {
        let mut app = App::new();
        let panel = app.world_mut().spawn_empty().id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, app.world());
        super::build_network_cache_tab(&mut commands, panel);
        queue.apply(app.world_mut());
        let mut rows = app
            .world_mut()
            .query::<&crate::preferences::PrefSearchRow>();
        assert_eq!(rows.iter(app.world()).count(), 9, "9 searchable rows");
    }

    /// Every row / section / button Fluent key this tab spawns is distinct.
    #[test]
    fn tab_label_keys_are_distinct() {
        let keys = [
            "preferences-tab-network-cache",
            "preferences-section-network",
            "preferences-section-proxy",
            "preferences-section-cache",
            "preferences-row-max-bandwidth",
            "preferences-row-http-proxy-enabled",
            "preferences-row-http-proxy",
            "preferences-row-texture-cache-size",
            "preferences-row-asset-cache-size",
            "preferences-row-cache-location",
            "preferences-row-chat-log-location",
            "preferences-row-clear-cache",
            "preferences-row-clear-inventory-cache",
            "preferences-clear-cache",
            "preferences-clear-inventory-cache",
            "preferences-reset-default",
        ];
        let distinct: std::collections::BTreeSet<&str> = keys.iter().copied().collect();
        assert_eq!(distinct.len(), keys.len(), "duplicate Fluent key");
    }

    /// Deleting inventory snapshots removes exactly the `.inv.llsd.gz` /
    /// `.lib.inv.llsd.gz` files, leaving the tree and other files alone.
    #[test]
    fn inventory_cache_delete_targets_only_snapshots() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after the epoch")
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "{}-invcache-{nanos}-{:?}",
            env!("CARGO_PKG_NAME"),
            std::thread::current().id()
        ));
        let avatar = base.join("grid/avatar");
        fs_err::create_dir_all(&avatar).expect("seed tree");
        fs_err::write(avatar.join("a.inv.llsd.gz"), b"x").expect("seed agent snapshot");
        fs_err::write(avatar.join("a.lib.inv.llsd.gz"), b"x").expect("seed library snapshot");
        fs_err::write(avatar.join("unrelated.txt"), b"x").expect("seed bystander");

        super::delete_inventory_caches(&base);

        assert!(!avatar.join("a.inv.llsd.gz").exists());
        assert!(!avatar.join("a.lib.inv.llsd.gz").exists());
        assert!(avatar.join("unrelated.txt").exists());

        fs_err::remove_dir_all(&base).expect("cleanup");
    }
}
