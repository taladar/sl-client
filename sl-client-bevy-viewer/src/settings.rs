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
    /// The accounts root (`<data>/accounts`), or `None` when the platform has no
    /// data directory (per-avatar persistence is then disabled).
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
}

impl FromWorld for ViewerSettings {
    /// Build the store, register every feature's settings, and load any saved
    /// global overrides. The account scope loads later, at login
    /// ([`load_account_settings`]).
    fn from_world(_world: &mut World) -> Self {
        let mut settings = Self {
            store: SettingsStore::new(),
            global_path: crate::paths::global_settings_file(),
            account_path: None,
        };
        crate::spacenav::register_settings(&mut settings);
        settings.load_global();
        settings
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
