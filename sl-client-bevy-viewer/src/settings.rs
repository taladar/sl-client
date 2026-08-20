//! The viewer's persistent settings store (`viewer-ui-settings-store` wired into
//! the app): a thin Bevy resource over [`sl_settings::SettingsStore`], the
//! reference viewer's `gSavedSettings` counterpart.
//!
//! Only the settings the current features need are registered here; more join as
//! their features land. The file is TOML: each override is a commented
//! `name = value` line grouped into a `[section]` table.
//!
//! Two scopes are persisted, mirroring the reference viewer's `gSavedSettings` /
//! `gSavedPerAccountSettings` split:
//!
//! - The [`Global`](Scope::Global) scope loads from and saves to the platform
//!   config directory's `viewer-settings.toml` ([`crate::paths`]).
//! - The [`Account`](Scope::Account) scope is per-avatar: once the agent UUID is
//!   known at login, [`load_account_settings`] resolves the avatar's directory
//!   (keyed by grid + avatar name, with rename discovery — [`sl_account_dirs`])
//!   and loads its `settings.toml`. It resolves over the global scope.
//!
//! Both scopes save on a clean logout, so a tuned value (e.g. a SpaceNavigator
//! sensitivity) survives a restart.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::IoTaskPool;
use sl_client_bevy::SlIdentity;
use sl_settings::{Scope, SettingValue, SettingsStore};
use tracing::{info, warn};

/// The account-scope settings filename within a per-avatar account directory.
const ACCOUNT_SETTINGS_FILE: &str = "settings.toml";

/// The per-avatar account identity resolved from the credentials before login
/// (grid + readable avatar name) and the accounts root, used to locate the
/// account settings directory once the agent UUID is known at login.
#[derive(Resource, Clone)]
pub(crate) struct AccountContext {
    /// The accounts root for settings (`<config>/accounts`), or `None` when the
    /// platform has no config directory (per-avatar settings are then disabled).
    pub accounts_base: Option<PathBuf>,
    /// The grid segment (from `sl_account_dirs::grid_dir_name`).
    pub grid: String,
    /// The readable avatar segment (from `sl_account_dirs::avatar_dir_name`).
    pub avatar: String,
}

/// The viewer's settings store, a Bevy resource.
#[derive(Resource)]
pub(crate) struct ViewerSettings {
    /// The underlying typed store (declared defaults + global/account overrides).
    store: SettingsStore,
    /// Where the global scope is persisted (the platform config directory).
    global_path: PathBuf,
    /// Where the account scope is persisted, once resolved at login; `None`
    /// until then (and when the platform has no per-avatar directory).
    account_path: Option<PathBuf>,
}

impl ViewerSettings {
    /// A read-only view of the store, for consumers reading their settings.
    #[must_use]
    pub(crate) const fn store(&self) -> &SettingsStore {
        &self.store
    }

    /// Register a setting's declared default (name → value + comment) under a
    /// section, logging and swallowing the (only-on-duplicate) error so a double
    /// registration can never abort startup.
    fn declare(&mut self, section: &[&str], name: &str, value: SettingValue, comment: &str) {
        if let Err(error) = self.store.register_in(section, name, value, comment) {
            warn!("settings: could not register {name}: {error}");
        }
    }

    /// Register a setting grouped under a `[section]` of the persisted file
    /// (e.g. `["spacenav", "flycam"]` → `[spacenav.flycam]`), the pub(crate)
    /// entry a feature module calls from [`FromWorld`]. Pass an empty section to
    /// place the setting at the document root.
    pub(crate) fn register_in(
        &mut self,
        section: &[&str],
        name: &str,
        value: SettingValue,
        comment: &str,
    ) {
        self.declare(section, name, value, comment);
    }

    /// Register a persisted setting grouped under a `[section]` that the raw
    /// debug-settings editor **skips** — mechanical UI state (window geometry,
    /// tab splits, table sort orders) that is saved and restored but is not a
    /// knob anyone debugs by hand. The floater / table persistence layers call
    /// this instead of [`register_in`](Self::register_in).
    pub(crate) fn register_hidden_in(
        &mut self,
        section: &[&str],
        name: &str,
        value: SettingValue,
        comment: &str,
    ) {
        if let Err(error) = self.store.register_hidden_in(section, name, value, comment) {
            warn!("settings: could not register {name}: {error}");
        }
    }

    /// Register a runtime-only setting whose overrides are never persisted (the
    /// reference viewer's transient debug settings). The two-way binding demo
    /// ([`crate::settings_binding`]) uses this so its scratch values write no junk
    /// to the user's config.
    pub(crate) fn register_transient(&mut self, name: &str, value: SettingValue, comment: &str) {
        if let Err(error) = self.store.register_transient(name, value, comment) {
            warn!("settings: could not register {name}: {error}");
        }
    }

    /// Replace a registered setting's declared default (see
    /// [`SettingsStore::set_default`](sl_settings::SettingsStore::set_default)),
    /// logging and swallowing a (wrong-type or unregistered) error so a bad
    /// dynamic default can never abort a frame. The skin-colour bridge
    /// ([`crate::skin_colors`]) feeds the active skin's palette in here.
    pub(crate) fn set_default(&mut self, name: &str, value: SettingValue) {
        if let Err(error) = self.store.set_default(name, value) {
            warn!("settings: could not set default for {name}: {error}");
        }
    }

    /// Write a value to the per-avatar [`Account`](Scope::Account) scope,
    /// logging and swallowing a (wrong-type or unregistered) error so a bad write
    /// can never abort a frame. The floater-geometry persistence
    /// ([`crate::floater_persist`]) writes each floater's remembered rect /
    /// visibility here as it changes.
    pub(crate) fn set_account(&mut self, name: &str, value: SettingValue) {
        self.set(Scope::Account, name, value);
    }

    /// Write a value to a chosen override scope, logging and swallowing a
    /// (wrong-type or unregistered) error so a bad write can never abort a
    /// frame. The two-way widget binding ([`crate::settings_binding`]) writes a
    /// user edit here, at the binding's declared scope.
    pub(crate) fn set(&mut self, scope: Scope, name: &str, value: SettingValue) {
        if let Err(error) = self.store.set(scope, name, value) {
            warn!("settings: could not set {name}: {error}");
        }
    }

    /// Drop a setting's override in one scope, reverting it to the layer below
    /// (see [`SettingsStore::reset`](sl_settings::SettingsStore::reset)). Returns
    /// whether an override was actually present. A bound "reset to default"
    /// control ([`crate::settings_binding`]) calls this.
    pub(crate) fn reset(&mut self, scope: Scope, name: &str) -> bool {
        self.store.reset(scope, name)
    }

    /// Whether the per-avatar account scope has been resolved and loaded (post
    /// login). Consumers that seed themselves from a saved *account* value wait
    /// for this, since the account overrides are not in the store until then.
    #[must_use]
    pub(crate) const fn account_loaded(&self) -> bool {
        self.account_path.is_some()
    }

    /// The per-avatar account directory (the parent of the account
    /// `settings.toml`), once resolved at login — where a sibling per-account file
    /// (the persistent-notification store, [`crate::notification_persist`]) lives.
    /// `None` until login, and when the platform has no per-avatar directory.
    #[must_use]
    pub(crate) fn account_dir(&self) -> Option<&Path> {
        self.account_path.as_deref().and_then(Path::parent)
    }

    /// Load the persisted global overrides, if the file exists — a missing file is
    /// the common first-run case and not an error.
    fn load_global(&mut self) {
        if !Path::new(&self.global_path).exists() {
            return;
        }
        match self.store.load_scope(Scope::Global, &self.global_path) {
            Ok(_loaded) => info!("settings: loaded {}", self.global_path.display()),
            Err(error) => warn!(
                "settings: could not load {}: {error}",
                self.global_path.display()
            ),
        }
    }

    /// Load the per-avatar account overrides from `account_dir/settings.toml`,
    /// recording the path so they are saved back there on logout. A missing file
    /// is the first-run case for that avatar and not an error.
    fn load_account(&mut self, account_dir: &Path) {
        let path = account_dir.join(ACCOUNT_SETTINGS_FILE);
        match self.store.load_scope(Scope::Account, &path) {
            Ok(_loaded) => info!("settings: loaded account scope {}", path.display()),
            Err(error) => warn!("settings: could not load {}: {error}", path.display()),
        }
        self.account_path = Some(path);
    }

    /// Save the global (and, once resolved, account) overrides to disk
    /// (best-effort; a failure is logged, not fatal). Called on a clean logout.
    pub(crate) fn save(&self) {
        if let Err(error) = self.store.save_scope(Scope::Global, &self.global_path) {
            warn!(
                "settings: could not save {}: {error}",
                self.global_path.display()
            );
        }
        if let Some(path) = &self.account_path
            && let Err(error) = self.store.save_scope(Scope::Account, path)
        {
            warn!("settings: could not save {}: {error}", path.display());
        }
    }

    /// Save both scopes without blocking the frame on disk I/O: serialize on
    /// the calling (frame) thread — the TOML is small — and write the files on
    /// a detached [`IoTaskPool`] task, logging (never hiding) write failures.
    /// For the in-session persistence paths (floater geometry flush, table
    /// column widths, preferences apply, list sort changes); the logout / exit
    /// path keeps the synchronous [`save`](Self::save), where a detached write
    /// racing process exit could be lost.
    pub(crate) fn save_async(&self) {
        let mut writes: Vec<(String, PathBuf)> = vec![(
            self.store.serialize_scope(Scope::Global),
            self.global_path.clone(),
        )];
        if let Some(path) = &self.account_path {
            writes.push((self.store.serialize_scope(Scope::Account), path.clone()));
        }
        IoTaskPool::get()
            .spawn(async move {
                for (text, path) in writes {
                    match fs_err::write(&path, text) {
                        Ok(()) => info!("settings: saved {}", path.display()),
                        Err(error) => {
                            warn!("settings: could not save {}: {error}", path.display());
                        }
                    }
                }
            })
            .detach();
    }

    /// Build a store-backed resource with no persistence paths, for unit tests
    /// that drive the store directly (e.g. the two-way binding tests) without
    /// touching the filesystem.
    #[cfg(test)]
    pub(crate) const fn from_store_for_test(store: SettingsStore) -> Self {
        Self {
            store,
            global_path: PathBuf::new(),
            account_path: None,
        }
    }

    /// Pretend the account scope has loaded (an empty in-memory path), so a
    /// test can exercise a post-login gate ([`Self::account_loaded`], the
    /// preferences floater's account guard) without a filesystem.
    #[cfg(test)]
    pub(crate) fn mark_account_loaded_for_test(&mut self) {
        self.account_path = Some(PathBuf::new());
    }

    /// Build the store, register every feature's settings, and load any saved
    /// global overrides. The account scope loads later, at login
    /// ([`load_account_settings`]). Split out of [`FromWorld`] so pre-app code
    /// (the login-request construction in `run_viewer`, which needs the stored
    /// start location before the Bevy [`World`] exists) reads the same store.
    pub(crate) fn load() -> Self {
        let mut settings = Self {
            store: SettingsStore::new(),
            global_path: crate::paths::global_settings_file(),
            account_path: None,
        };
        crate::spacenav::register_settings(&mut settings);
        crate::minimap::register_settings(&mut settings);
        crate::double_click_teleport::register_settings(&mut settings);
        crate::parcel_borders::register_settings(&mut settings);
        crate::world_map::register_settings(&mut settings);
        crate::search::register_settings(&mut settings);
        crate::tonemap::register_settings(&mut settings);
        crate::glow::register_settings(&mut settings);
        crate::exposure::register_settings(&mut settings);
        crate::snapshot_floater::register_settings(&mut settings);
        crate::i18n::register_settings(&mut settings);
        crate::avatars::register_settings(&mut settings);
        crate::hover_text::register_settings(&mut settings);
        crate::hover_tooltip::register_settings(&mut settings);
        crate::preferences_camera_move::register_settings(&mut settings);
        crate::preferences_chat::register_settings(&mut settings);
        crate::preferences_colors_skins::register_settings(&mut settings);
        crate::preferences_general::register_settings(&mut settings);
        crate::preferences_graphics::register_settings(&mut settings);
        crate::preferences_network_cache::register_settings(&mut settings);
        crate::presence::register_settings(&mut settings);
        crate::skin_colors::register_settings(&mut settings);
        crate::session::register_settings(&mut settings);
        crate::render_priority::register_settings(&mut settings);
        crate::particles::register_settings(&mut settings);
        crate::ui_sounds::register_settings(&mut settings);
        crate::audio::register_settings(&mut settings);
        crate::debug_settings::register_settings(&mut settings);
        settings.load_global();
        settings
    }
}
impl FromWorld for ViewerSettings {
    /// [`ViewerSettings::load`], as the resource initialiser.
    fn from_world(_world: &mut World) -> Self {
        Self::load()
    }
}

/// Once the agent UUID is known (post-login), resolve the per-avatar account
/// directory — keyed by grid + avatar name, renaming it in place if the UUID
/// shows a name change — and load its account-scope settings. Runs every frame
/// but does its work exactly once (guarded on `account_path` being unset).
pub(crate) fn load_account_settings(
    mut settings: ResMut<ViewerSettings>,
    context: Res<AccountContext>,
    identity: Res<SlIdentity>,
) {
    // Already loaded, not logged in yet, or no per-avatar directory available.
    if settings.account_path.is_some() {
        return;
    }
    let Some(agent) = identity.agent_id else {
        return;
    };
    let Some(base) = context.accounts_base.clone() else {
        return;
    };
    match sl_account_dirs::reconcile_account_dir(
        &base,
        &context.grid,
        &context.avatar,
        agent.uuid(),
    ) {
        Ok(dir) => settings.load_account(&dir),
        Err(error) => warn!(
            "settings: could not resolve account directory under {}: {error}",
            base.display()
        ),
    }
}
