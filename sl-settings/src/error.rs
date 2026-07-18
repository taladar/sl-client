//! The error type the settings store's fallible operations return.

use crate::value::SettingKind;

/// Why a settings-store operation failed.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_settings::SettingError`, where it reads clearly"
)]
#[derive(Debug, thiserror::Error)]
pub enum SettingError {
    /// A read or write named a setting that was never [`register`]ed.
    ///
    /// [`register`]: crate::SettingsStore::register
    #[error("no setting named {0:?} is registered")]
    UnknownSetting(String),
    /// A setting with this name was already registered — a declaration is a
    /// one-time, program-startup action, so a second one is a bug.
    #[error("a setting named {0:?} is already registered")]
    AlreadyRegistered(String),
    /// A value of the wrong type was written to (or read from) a setting: the
    /// setting was declared `expected` but the operation used `found`.
    #[error("setting {name:?} has type {expected:?}, not {found:?}")]
    TypeMismatch {
        /// The name of the setting whose type did not match.
        name: String,
        /// The type the setting was declared with.
        expected: SettingKind,
        /// The type the mismatching value had.
        found: SettingKind,
    },
    /// Reading or writing a persistence file failed.
    #[error("settings file I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// A persistence file's contents were not the expected TOML.
    #[error("settings file did not parse as TOML: {0}")]
    Toml(#[from] toml_edit::TomlError),
}
