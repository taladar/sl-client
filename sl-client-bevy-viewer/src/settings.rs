//! The viewer's persistent settings store (`viewer-ui-settings-store` wired into
//! the app): a thin Bevy resource over [`sl_settings::SettingsStore`], the
//! reference viewer's `gSavedSettings` counterpart.
//!
//! Only the settings the current features need are registered here; more join as
//! their features land. The store loads its [`Global`](sl_settings::Scope::Global)
//! overrides from a `viewer-settings.toml` at startup and saves them on a clean
//! logout, so a tuned value (e.g. a SpaceNavigator sensitivity) survives a
//! restart. The file is TOML: each override is a commented `name = value` line
//! grouped into a `[section]` table. The account layer (a separate per-avatar
//! file, still unwired — see `viewer-settings-account-scope-persist`) and the
//! two-way widget binding (`viewer-input-spacenav-settings-ui`) build on top of
//! this.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use sl_settings::{Scope, SettingValue, SettingsStore};
use tracing::{info, warn};

/// The file the [`Global`](Scope::Global) settings scope is loaded from and saved
/// to, relative to the working directory (the same place the credentials file
/// lives by default). TOML, per the store's serialisation.
const SETTINGS_FILE: &str = "viewer-settings.toml";

/// The viewer's settings store, a Bevy resource.
#[derive(Resource)]
pub(crate) struct ViewerSettings {
    /// The underlying typed store (declared defaults + global/account overrides).
    store: SettingsStore,
    /// Where the global scope is persisted.
    path: PathBuf,
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
    fn load(&mut self) {
        if !Path::new(&self.path).exists() {
            return;
        }
        match self.store.load_scope(Scope::Global, &self.path) {
            Ok(_loaded) => info!("settings: loaded {}", self.path.display()),
            Err(error) => warn!("settings: could not load {}: {error}", self.path.display()),
        }
    }

    /// Save the global overrides to disk (best-effort; a failure is logged, not
    /// fatal). Called on a clean logout.
    pub(crate) fn save(&self) {
        if let Err(error) = self.store.save_scope(Scope::Global, &self.path) {
            warn!("settings: could not save {}: {error}", self.path.display());
        }
    }
}

impl FromWorld for ViewerSettings {
    /// Build the store, register every feature's settings, and load any saved
    /// global overrides.
    fn from_world(_world: &mut World) -> Self {
        let mut settings = Self {
            store: SettingsStore::new(),
            path: PathBuf::from(SETTINGS_FILE),
        };
        crate::spacenav::register_settings(&mut settings);
        settings.load();
        settings
    }
}
