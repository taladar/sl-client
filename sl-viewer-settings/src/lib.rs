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
//!   config directory's `viewer-settings.toml` (`paths`).
//! - The [`Account`](Scope::Account) scope is per-avatar: once the agent UUID is
//!   known at login, [`load_account_settings`] resolves the avatar's directory
//!   (keyed by grid + avatar name, with rename discovery — [`sl_account_dirs`])
//!   and loads its `settings.toml`. It resolves over the global scope.
//!
//! Both scopes save on a clean logout, so a tuned value (e.g. a SpaceNavigator
//! sensitivity) survives a restart.
//!
//! [`keys`] holds the setting **names** that both the feature owning a setting
//! and the preferences panel drawing a control for it have to agree on — the
//! layer beneath both, so neither has to depend on the other for a string.

pub mod keys;

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
#[derive(Debug, Resource, Clone)]
pub struct AccountContext {
    /// The accounts root for settings (`<config>/accounts`), or `None` when the
    /// platform has no config directory (per-avatar settings are then disabled).
    pub accounts_base: Option<PathBuf>,
    /// The grid segment (from `sl_account_dirs::grid_dir_name`).
    pub grid: String,
    /// The readable avatar segment (from `sl_account_dirs::avatar_dir_name`).
    pub avatar: String,
}

/// The viewer's settings store, a Bevy resource.
#[derive(Debug, Resource)]
pub struct ViewerSettings {
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
    pub const fn store(&self) -> &SettingsStore {
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
    pub fn register_in(
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
    pub fn register_hidden_in(
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
    /// (`settings_binding`) uses this so its scratch values write no junk
    /// to the user's config.
    pub fn register_transient(&mut self, name: &str, value: SettingValue, comment: &str) {
        if let Err(error) = self.store.register_transient(name, value, comment) {
            warn!("settings: could not register {name}: {error}");
        }
    }

    /// Replace a registered setting's declared default (see
    /// [`SettingsStore::set_default`](sl_settings::SettingsStore::set_default)),
    /// logging and swallowing a (wrong-type or unregistered) error so a bad
    /// dynamic default can never abort a frame. The skin-colour bridge
    /// (`skin_colors`) feeds the active skin's palette in here.
    pub fn set_default(&mut self, name: &str, value: SettingValue) {
        if let Err(error) = self.store.set_default(name, value) {
            warn!("settings: could not set default for {name}: {error}");
        }
    }

    /// Write a value to the per-avatar [`Account`](Scope::Account) scope,
    /// logging and swallowing a (wrong-type or unregistered) error so a bad write
    /// can never abort a frame. The floater-geometry persistence
    /// (`floater_persist`) writes each floater's remembered rect /
    /// visibility here as it changes.
    pub fn set_account(&mut self, name: &str, value: SettingValue) {
        self.set(Scope::Account, name, value);
    }

    /// Write a value to a chosen override scope, logging and swallowing a
    /// (wrong-type or unregistered) error so a bad write can never abort a
    /// frame. The two-way widget binding (`settings_binding`) writes a
    /// user edit here, at the binding's declared scope.
    pub fn set(&mut self, scope: Scope, name: &str, value: SettingValue) {
        if let Err(error) = self.store.set(scope, name, value) {
            warn!("settings: could not set {name}: {error}");
        }
    }

    /// Drop a setting's override in one scope, reverting it to the layer below
    /// (see [`SettingsStore::reset`](sl_settings::SettingsStore::reset)). Returns
    /// whether an override was actually present. A bound "reset to default"
    /// control (`settings_binding`) calls this.
    pub fn reset(&mut self, scope: Scope, name: &str) -> bool {
        self.store.reset(scope, name)
    }

    /// Whether the per-avatar account scope has been resolved and loaded (post
    /// login). Consumers that seed themselves from a saved *account* value wait
    /// for this, since the account overrides are not in the store until then.
    #[must_use]
    pub const fn account_loaded(&self) -> bool {
        self.account_path.is_some()
    }

    /// The per-avatar account directory (the parent of the account
    /// `settings.toml`), once resolved at login — where a sibling per-account file
    /// (the persistent-notification store, `notification_persist`) lives.
    /// `None` until login, and when the platform has no per-avatar directory.
    #[must_use]
    pub fn account_dir(&self) -> Option<&Path> {
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
    pub fn save(&self) {
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
    pub fn save_async(&self) {
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
    ///
    /// Not `#[cfg(test)]`: the viewer's own tests build settings this way, and
    /// a `cfg(test)` item is not compiled when a *dependent* crate runs its
    /// tests. The `_for_test` name is the documentation — nothing in a running
    /// viewer should call it.
    #[must_use]
    pub const fn from_store_for_test(store: SettingsStore) -> Self {
        Self {
            store,
            global_path: PathBuf::new(),
            account_path: None,
        }
    }

    /// Pretend the account scope has loaded (an empty in-memory path), so a
    /// test can exercise a post-login gate ([`Self::account_loaded`], the
    /// preferences floater's account guard) without a filesystem.
    ///
    /// Not `#[cfg(test)]`: the viewer's own tests build settings this way, and
    /// a `cfg(test)` item is not compiled when a *dependent* crate runs its
    /// tests. The `_for_test` name is the documentation — nothing in a running
    /// viewer should call it.
    pub fn mark_account_loaded_for_test(&mut self) {
        self.account_path = Some(PathBuf::new());
    }

    /// Build the store with no registrations and no file load — the shared
    /// starting point for [`load_with`](Self::load_with) and for a test that
    /// wants to inspect what a given set of registrars declares.
    fn empty(global_path: PathBuf) -> Self {
        Self {
            store: SettingsStore::new(),
            global_path,
            account_path: None,
        }
    }

    /// Build the store, run every `registrars` entry, and load any saved global
    /// overrides. The account scope loads later, at login
    /// ([`load_account_settings`]).
    ///
    /// The registrar list is supplied by the caller rather than named here,
    /// because this store cannot know the features that use it without
    /// depending on all of them. The viewer passes its full list; see
    /// `REGISTRARS` in the binary crate.
    ///
    /// Not a [`FromWorld`] initializer, because pre-app code (the login-request
    /// construction in `run_viewer`, which needs the stored start location
    /// before the Bevy [`World`] exists) reads the same store.
    pub fn load_with(registrars: &[fn(&mut Self)]) -> Self {
        let mut settings = Self::empty(sl_viewer_platform::paths::global_settings_file());
        settings.run_registrars(registrars);
        settings.load_global();
        settings
    }

    /// Run each registrar against this store, in the order given.
    pub fn run_registrars(&mut self, registrars: &[fn(&mut Self)]) {
        for register in registrars {
            register(self);
        }
    }

    /// A store with `registrars` applied and nothing loaded from disk, so a
    /// test can compare the declared surface without touching the filesystem.
    /// Not `#[cfg(test)]`: the viewer's own tests build settings this way,
    /// and a `cfg(test)` item is not compiled when a *dependent* crate runs
    /// its tests. The `_for_test` name is the documentation — nothing in a
    /// running viewer should call it.
    pub fn declared_for_test(registrars: &[fn(&mut Self)]) -> Self {
        let mut settings = Self::empty(PathBuf::new());
        settings.run_registrars(registrars);
        settings
    }
}

/// Once the agent UUID is known (post-login), resolve the per-avatar account
/// directory — keyed by grid + avatar name, renaming it in place if the UUID
/// shows a name change — and load its account-scope settings. Runs every frame
/// but does its work exactly once (guarded on `account_path` being unset).
pub fn load_account_settings(
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
