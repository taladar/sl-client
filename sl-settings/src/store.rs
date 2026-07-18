//! The settings store itself: declarations, the layered override scopes, typed
//! access, and TOML persistence.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::SettingError;
use crate::toml_format::{self, Entry};
use crate::value::{SettingKind, SettingValue};

/// Which override layer a value is read from or written to.
///
/// The effective value of a setting is resolved by consulting the layers in
/// order — an [`Account`](Scope::Account) override wins over a
/// [`Global`](Scope::Global) override, which wins over the declared default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The machine-wide (per-install) override layer, shared by every account.
    Global,
    /// The per-account override layer, loaded when an account logs in and
    /// cleared when it logs out.
    Account,
}

/// A setting's declaration: the default value that fixes its type, a
/// human-readable comment, and whether it is written to disk.
///
/// Declarations are registered once at program start and are the source of
/// truth for every setting's type and default; the reference viewer's
/// equivalent is a control loaded from `settings.xml`.
#[derive(Debug, Clone)]
pub struct SettingDecl {
    /// The default value, returned when no override is present. Its
    /// [`SettingValue::kind`] fixes the setting's type.
    default: SettingValue,
    /// A short human-readable description, shown by a raw settings editor and
    /// written as the comment above the setting in the persisted TOML.
    comment: String,
    /// The section path the setting is grouped under in the persisted TOML
    /// (e.g. `["spacenav", "flycam"]` → `[spacenav.flycam]`). Empty places the
    /// setting at the document root.
    section: Vec<String>,
    /// Whether overrides of this setting are persisted to disk. A transient
    /// (runtime-only) setting sets this `false`.
    persist: bool,
}

impl SettingDecl {
    /// The default value returned when no override is set.
    #[must_use]
    pub const fn default(&self) -> &SettingValue {
        &self.default
    }

    /// The declared type of this setting.
    #[must_use]
    pub const fn kind(&self) -> SettingKind {
        self.default.kind()
    }

    /// The human-readable description.
    #[must_use]
    pub fn comment(&self) -> &str {
        &self.comment
    }

    /// The section path this setting is grouped under in the persisted file.
    #[must_use]
    pub fn section(&self) -> &[String] {
        &self.section
    }

    /// Whether overrides of this setting are written to disk.
    #[must_use]
    pub const fn persist(&self) -> bool {
        self.persist
    }
}

/// A typed, persistent settings store.
///
/// Settings are [`register`](SettingsStore::register)ed once with a typed
/// default, then read and written by name. Reads resolve through the
/// [`Scope`] layers (account → global → default); writes target one scope. Each
/// scope's overrides load from and save to a TOML file independently, so the
/// global file is shared while a per-account file is swapped on login.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_settings::SettingsStore`, where it reads clearly"
)]
#[derive(Debug, Clone, Default)]
pub struct SettingsStore {
    /// Every registered setting, keyed by name (sorted for a stable editor
    /// order).
    decls: BTreeMap<String, SettingDecl>,
    /// The global override layer.
    global: BTreeMap<String, SettingValue>,
    /// The per-account override layer.
    account: BTreeMap<String, SettingValue>,
}

impl SettingsStore {
    /// An empty store with no settings registered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a persisted setting with a typed default and a description, at
    /// the document root of the persisted file.
    ///
    /// The `default`'s type fixes the setting's type: later writes of a
    /// different type are rejected. Registering a name twice is an error, since
    /// declaration is a one-time startup action. Use
    /// [`register_in`](SettingsStore::register_in) to group the setting under a
    /// section.
    ///
    /// # Errors
    ///
    /// [`SettingError::AlreadyRegistered`] if `name` is already registered.
    pub fn register(
        &mut self,
        name: impl Into<String>,
        default: SettingValue,
        comment: impl Into<String>,
    ) -> Result<(), SettingError> {
        self.declare(name.into(), default, comment.into(), Vec::new(), true)
    }

    /// Register a persisted setting grouped under a section of the persisted
    /// file (e.g. `["spacenav", "flycam"]` → `[spacenav.flycam]`).
    ///
    /// Otherwise identical to [`register`](SettingsStore::register).
    ///
    /// # Errors
    ///
    /// [`SettingError::AlreadyRegistered`] if `name` is already registered.
    pub fn register_in(
        &mut self,
        section: &[&str],
        name: impl Into<String>,
        default: SettingValue,
        comment: impl Into<String>,
    ) -> Result<(), SettingError> {
        self.declare(
            name.into(),
            default,
            comment.into(),
            section_path(section),
            true,
        )
    }

    /// Register a transient (runtime-only, never persisted) setting.
    ///
    /// Behaves like [`register`](SettingsStore::register) but its overrides are
    /// skipped by [`save_scope`](SettingsStore::save_scope), so its section is
    /// irrelevant.
    ///
    /// # Errors
    ///
    /// [`SettingError::AlreadyRegistered`] if `name` is already registered.
    pub fn register_transient(
        &mut self,
        name: impl Into<String>,
        default: SettingValue,
        comment: impl Into<String>,
    ) -> Result<(), SettingError> {
        self.declare(name.into(), default, comment.into(), Vec::new(), false)
    }

    /// Shared body of the `register*` methods.
    fn declare(
        &mut self,
        name: String,
        default: SettingValue,
        comment: String,
        section: Vec<String>,
        persist: bool,
    ) -> Result<(), SettingError> {
        if self.decls.contains_key(&name) {
            return Err(SettingError::AlreadyRegistered(name));
        }
        let decl = SettingDecl {
            default,
            comment,
            section,
            persist,
        };
        let _prev = self.decls.insert(name, decl);
        Ok(())
    }

    /// The declaration of a registered setting, or `None` if it is unknown.
    #[must_use]
    pub fn declaration(&self, name: &str) -> Option<&SettingDecl> {
        self.decls.get(name)
    }

    /// The names of every registered setting, in sorted order.
    ///
    /// A raw settings editor iterates this to list every setting generically.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.decls.keys().map(String::as_str)
    }

    /// The effective value of a setting: the account override if present, else
    /// the global override, else the declared default. `None` if the name is
    /// neither registered nor overridden.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SettingValue> {
        self.account
            .get(name)
            .or_else(|| self.global.get(name))
            .or_else(|| self.decls.get(name).map(SettingDecl::default))
    }

    /// Write a value to a scope's override layer.
    ///
    /// The value's type must match the setting's declared type.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if the setting is not registered, or
    /// [`SettingError::TypeMismatch`] if `value` has the wrong type.
    pub fn set(
        &mut self,
        scope: Scope,
        name: &str,
        value: SettingValue,
    ) -> Result<(), SettingError> {
        let Some(decl) = self.decls.get(name) else {
            return Err(SettingError::UnknownSetting(name.to_owned()));
        };
        let expected = decl.kind();
        let found = value.kind();
        if expected != found {
            return Err(SettingError::TypeMismatch {
                name: name.to_owned(),
                expected,
                found,
            });
        }
        let _prev = self.scope_map_mut(scope).insert(name.to_owned(), value);
        Ok(())
    }

    /// Remove a setting's override in one scope, reverting it to the next layer
    /// down. Returns whether an override was actually present.
    pub fn reset(&mut self, scope: Scope, name: &str) -> bool {
        self.scope_map_mut(scope).remove(name).is_some()
    }

    /// Drop every override in a scope (e.g. the account layer on logout).
    pub fn clear_scope(&mut self, scope: Scope) {
        self.scope_map_mut(scope).clear();
    }

    /// The override map for a scope.
    const fn scope_map(&self, scope: Scope) -> &BTreeMap<String, SettingValue> {
        match scope {
            Scope::Global => &self.global,
            Scope::Account => &self.account,
        }
    }

    /// The mutable override map for a scope.
    const fn scope_map_mut(&mut self, scope: Scope) -> &mut BTreeMap<String, SettingValue> {
        match scope {
            Scope::Global => &mut self.global,
            Scope::Account => &mut self.account,
        }
    }

    /// Write a scope's persistable overrides to a TOML file, creating or
    /// truncating it.
    ///
    /// Each override is written as a commented `name = value` line, grouped into
    /// its declared section (see [`register_in`](SettingsStore::register_in));
    /// the value's type is not written, since it is fixed by the declaration.
    /// Overrides of transient settings (and only those) are skipped; overrides
    /// of not-yet-registered settings are kept at the document root, so a value
    /// from a newer version is not lost on round-trip.
    ///
    /// # Errors
    ///
    /// [`SettingError::Io`] if the file cannot be written.
    pub fn save_scope(&self, scope: Scope, path: impl AsRef<Path>) -> Result<(), SettingError> {
        let mut entries: Vec<Entry<'_>> = Vec::new();
        for (name, value) in self.scope_map(scope) {
            match self.decls.get(name) {
                Some(decl) if decl.persist() => entries.push(Entry {
                    name,
                    value,
                    section: decl.section(),
                    comment: decl.comment(),
                }),
                // A transient setting is never persisted.
                Some(_decl) => {}
                // An override of a setting this build does not declare is kept
                // (forward compatibility), at the document root with no comment.
                None => entries.push(Entry {
                    name,
                    value,
                    section: &[],
                    comment: "",
                }),
            }
        }
        let text = toml_format::to_toml(&entries);
        fs_err::write(path, text)?;
        Ok(())
    }

    /// Load a scope's overrides from a TOML file, replacing that scope's current
    /// overrides.
    ///
    /// A missing file is not an error: the scope is left untouched and `false`
    /// is returned. Register the settings before loading — an entry whose value
    /// no longer fits a registered setting's declared type (a type changed
    /// across versions) is dropped, while an entry for a not-yet-registered
    /// setting is kept for forward compatibility.
    ///
    /// # Errors
    ///
    /// [`SettingError::Io`] for an I/O error other than the file being absent,
    /// or [`SettingError::Toml`] if the file is not valid TOML.
    pub fn load_scope(
        &mut self,
        scope: Scope,
        path: impl AsRef<Path>,
    ) -> Result<bool, SettingError> {
        let text = match fs_err::read_to_string(path.as_ref()) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let map =
            toml_format::from_toml(&text, &|name| self.decls.get(name).map(SettingDecl::kind))?;
        *self.scope_map_mut(scope) = map;
        Ok(true)
    }

    /// The effective boolean value of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not a boolean.
    pub fn get_bool(&self, name: &str) -> Result<bool, SettingError> {
        let value = self.require(name)?;
        value
            .as_bool()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::Bool))
    }

    /// The effective signed-integer value of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not a signed integer.
    pub fn get_i32(&self, name: &str) -> Result<i32, SettingError> {
        let value = self.require(name)?;
        value
            .as_i32()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::I32))
    }

    /// The effective unsigned-integer value of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not an unsigned integer.
    pub fn get_u32(&self, name: &str) -> Result<u32, SettingError> {
        let value = self.require(name)?;
        value
            .as_u32()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::U32))
    }

    /// The effective float value of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not a float.
    pub fn get_f32(&self, name: &str) -> Result<f32, SettingError> {
        let value = self.require(name)?;
        value
            .as_f32()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::F32))
    }

    /// The effective string value of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not a string.
    pub fn get_str(&self, name: &str) -> Result<&str, SettingError> {
        let value = self.require(name)?;
        value
            .as_str()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::String))
    }

    /// The effective RGB colour of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not an RGB colour.
    pub fn get_color3(&self, name: &str) -> Result<[f32; 3], SettingError> {
        let value = self.require(name)?;
        value
            .as_color3()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::Color3))
    }

    /// The effective RGBA colour of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not an RGBA colour.
    pub fn get_color4(&self, name: &str) -> Result<[f32; 4], SettingError> {
        let value = self.require(name)?;
        value
            .as_color4()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::Color4))
    }

    /// The effective `f32` 3-vector of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not an `f32` 3-vector.
    pub fn get_vec3(&self, name: &str) -> Result<[f32; 3], SettingError> {
        let value = self.require(name)?;
        value
            .as_vec3()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::Vec3))
    }

    /// The effective `f64` 3-vector of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not an `f64` 3-vector.
    pub fn get_vec3d(&self, name: &str) -> Result<[f64; 3], SettingError> {
        let value = self.require(name)?;
        value
            .as_vec3d()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::Vec3d))
    }

    /// The effective rectangle of a setting.
    ///
    /// # Errors
    ///
    /// [`SettingError::UnknownSetting`] if unknown, or
    /// [`SettingError::TypeMismatch`] if it is not a rectangle.
    pub fn get_rect(&self, name: &str) -> Result<[i32; 4], SettingError> {
        let value = self.require(name)?;
        value
            .as_rect()
            .ok_or_else(|| type_mismatch(name, value, SettingKind::Rect))
    }

    /// The effective value of a setting, or [`SettingError::UnknownSetting`].
    fn require(&self, name: &str) -> Result<&SettingValue, SettingError> {
        self.get(name)
            .ok_or_else(|| SettingError::UnknownSetting(name.to_owned()))
    }
}

/// Turn a borrowed section path into the owned form stored in a
/// [`SettingDecl`].
fn section_path(section: &[&str]) -> Vec<String> {
    section
        .iter()
        .map(|segment| (*segment).to_owned())
        .collect()
}

/// Build the [`SettingError::TypeMismatch`] for a typed getter whose
/// `requested` type did not match the stored `value`.
fn type_mismatch(name: &str, value: &SettingValue, requested: SettingKind) -> SettingError {
    SettingError::TypeMismatch {
        name: name.to_owned(),
        expected: value.kind(),
        found: requested,
    }
}
