//! A typed, persistent settings store for the Second Life / OpenSim viewer.
//!
//! Settings are named, typed and given sensible defaults up front; values are
//! then read and written by name and persisted to disk. Two override layers sit
//! over the declared defaults — a machine-wide [`Global`](Scope::Global) layer
//! and a [`Account`](Scope::Account) layer swapped per logged-in account — and
//! the effective value resolves account → global → default.
//!
//! This is the backend the UI binds to; the two-way widget binding on top of it
//! is a separate concern. It is the counterpart of the reference viewer's
//! `LLControlGroup` / `llviewercontrol` (global `gSavedSettings` +
//! per-account `gSavedPerAccountSettings`), but modelled on `serde` rather than
//! the reference's hand-rolled `LLInitParam` control serialization.
//!
//! # Example
//!
//! ```
//! # use sl_settings::{Scope, SettingValue, SettingsStore};
//! let mut store = SettingsStore::new();
//! store
//!     .register("RenderFarClip", SettingValue::F32(128.0), "Draw distance, m")
//!     .unwrap();
//!
//! // No override yet: the declared default is returned.
//! assert_eq!(store.get_f32("RenderFarClip").unwrap(), 128.0);
//!
//! // A per-account override wins over the global one.
//! store.set(Scope::Global, "RenderFarClip", SettingValue::F32(256.0)).unwrap();
//! store.set(Scope::Account, "RenderFarClip", SettingValue::F32(64.0)).unwrap();
//! assert_eq!(store.get_f32("RenderFarClip").unwrap(), 64.0);
//!
//! // Clearing the account override falls back to the global one.
//! assert!(store.reset(Scope::Account, "RenderFarClip"));
//! assert_eq!(store.get_f32("RenderFarClip").unwrap(), 256.0);
//! ```

pub mod error;
pub mod store;
mod toml_format;
pub mod value;

pub use error::SettingError;
pub use store::{Scope, SettingDecl, SettingsStore};
pub use value::{SettingKind, SettingValue};

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Scope, SettingError, SettingKind, SettingValue, SettingsStore};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Assert two `f32`s are equal within a small tolerance (an exact `==` on
    /// floats trips `clippy::float_cmp`).
    fn approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "{actual} != {expected} (within 1e-6)"
        );
    }

    /// Assert two `f32` slices are element-wise equal within tolerance.
    fn approx_slice(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len(), "length mismatch");
        for (got, want) in actual.iter().zip(expected) {
            approx(*got, *want);
        }
    }

    /// Assert two `f64` slices are element-wise equal within tolerance.
    fn approx_slice_f64(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "length mismatch");
        for (got, want) in actual.iter().zip(expected) {
            assert!((got - want).abs() < 1e-9, "{got} != {want} (within 1e-9)");
        }
    }

    /// A store with one setting of every type registered, for the type-checking
    /// and persistence tests.
    fn populated() -> Result<SettingsStore, SettingError> {
        let mut store = SettingsStore::new();
        store.register("Flag", SettingValue::Bool(true), "a toggle")?;
        store.register("Count", SettingValue::I32(-3), "a signed count")?;
        store.register("Size", SettingValue::U32(7), "an unsigned size")?;
        store.register("Clip", SettingValue::F32(128.0), "a float")?;
        store.register("Name", SettingValue::String("hi".to_owned()), "a string")?;
        store.register("Tint", SettingValue::Color3([0.1, 0.2, 0.3]), "an RGB")?;
        store.register(
            "Glow",
            SettingValue::Color4([0.1, 0.2, 0.3, 0.4]),
            "an RGBA",
        )?;
        store.register("Offset", SettingValue::Vec3([1.0, 2.0, 3.0]), "an f32 vec")?;
        store.register("Region", SettingValue::Vec3d([4.0, 5.0, 6.0]), "an f64 vec")?;
        store.register("Window", SettingValue::Rect([0, 100, 200, 0]), "a rect")?;
        Ok(store)
    }

    /// Every typed getter reads the declared default of its setting.
    #[test]
    fn typed_getters_read_defaults() -> Result<(), TestError> {
        let store = populated()?;
        assert!(store.get_bool("Flag")?);
        assert_eq!(store.get_i32("Count")?, -3);
        assert_eq!(store.get_u32("Size")?, 7);
        approx(store.get_f32("Clip")?, 128.0);
        assert_eq!(store.get_str("Name")?, "hi");
        approx_slice(&store.get_color3("Tint")?, &[0.1, 0.2, 0.3]);
        approx_slice(&store.get_color4("Glow")?, &[0.1, 0.2, 0.3, 0.4]);
        approx_slice(&store.get_vec3("Offset")?, &[1.0, 2.0, 3.0]);
        approx_slice_f64(&store.get_vec3d("Region")?, &[4.0, 5.0, 6.0]);
        assert_eq!(store.get_rect("Window")?, [0, 100, 200, 0]);
        Ok(())
    }

    /// Account overrides win over global overrides, which win over the default.
    #[test]
    fn scope_layering_resolves_account_first() -> Result<(), TestError> {
        let mut store = populated()?;
        assert_eq!(store.get_i32("Count")?, -3);

        store.set(Scope::Global, "Count", SettingValue::I32(10))?;
        assert_eq!(store.get_i32("Count")?, 10);

        store.set(Scope::Account, "Count", SettingValue::I32(20))?;
        assert_eq!(store.get_i32("Count")?, 20);

        // Resetting account falls back to global; resetting global to default.
        assert!(store.reset(Scope::Account, "Count"));
        assert_eq!(store.get_i32("Count")?, 10);
        assert!(store.reset(Scope::Global, "Count"));
        assert_eq!(store.get_i32("Count")?, -3);
        // Resetting an absent override reports nothing was there.
        assert!(!store.reset(Scope::Global, "Count"));
        Ok(())
    }

    /// Clearing the account scope drops every account override at once.
    #[test]
    fn clearing_account_scope_drops_all_overrides() -> Result<(), TestError> {
        let mut store = populated()?;
        store.set(Scope::Account, "Count", SettingValue::I32(99))?;
        store.set(Scope::Account, "Flag", SettingValue::Bool(false))?;
        assert_eq!(store.get_i32("Count")?, 99);

        store.clear_scope(Scope::Account);
        assert_eq!(store.get_i32("Count")?, -3);
        assert!(store.get_bool("Flag")?);
        Ok(())
    }

    /// Reading or writing an unregistered setting is a distinct error, not a
    /// silent default.
    #[test]
    fn unknown_setting_errors() -> Result<(), TestError> {
        let mut store = populated()?;
        assert!(matches!(
            store.get_i32("Nope"),
            Err(SettingError::UnknownSetting(name)) if name == "Nope"
        ));
        assert!(matches!(
            store.set(Scope::Global, "Nope", SettingValue::I32(1)),
            Err(SettingError::UnknownSetting(_))
        ));
        Ok(())
    }

    /// Writing a value of the wrong type, or reading through the wrong typed
    /// getter, is rejected.
    #[test]
    fn type_mismatch_errors() -> Result<(), TestError> {
        let mut store = populated()?;
        assert!(matches!(
            store.set(Scope::Global, "Count", SettingValue::Bool(true)),
            Err(SettingError::TypeMismatch {
                expected: SettingKind::I32,
                found: SettingKind::Bool,
                ..
            })
        ));
        assert!(matches!(
            store.get_bool("Count"),
            Err(SettingError::TypeMismatch {
                expected: SettingKind::I32,
                found: SettingKind::Bool,
                ..
            })
        ));
        Ok(())
    }

    /// Registering the same name twice is an error.
    #[test]
    fn duplicate_registration_errors() -> Result<(), TestError> {
        let mut store = SettingsStore::new();
        store.register("X", SettingValue::Bool(false), "first")?;
        assert!(matches!(
            store.register("X", SettingValue::Bool(true), "second"),
            Err(SettingError::AlreadyRegistered(name)) if name == "X"
        ));
        Ok(())
    }

    /// `names` lists exactly the registered settings, in sorted order.
    #[test]
    fn names_lists_registered_settings_sorted() -> Result<(), TestError> {
        let store = populated()?;
        let names: Vec<&str> = store.names().collect();
        assert_eq!(
            names,
            [
                "Clip", "Count", "Flag", "Glow", "Name", "Offset", "Region", "Size", "Tint",
                "Window",
            ]
        );
        Ok(())
    }

    /// Only the overridden, persistable settings round-trip through a save and
    /// reload; the reloaded store resolves them exactly as before.
    #[test]
    fn save_and_reload_round_trips_overrides() -> Result<(), TestError> {
        let dir = tempdir()?;
        let path = dir.join("global.toml");

        let mut store = populated()?;
        store.set(Scope::Global, "Clip", SettingValue::F32(256.0))?;
        store.set(
            Scope::Global,
            "Name",
            SettingValue::String("world".to_owned()),
        )?;
        store.save_scope(Scope::Global, &path)?;

        // A fresh store with the same declarations, loading the saved file.
        let mut reloaded = populated()?;
        assert!(reloaded.load_scope(Scope::Global, &path)?);
        approx(reloaded.get_f32("Clip")?, 256.0);
        assert_eq!(reloaded.get_str("Name")?, "world");
        // A setting that was never overridden keeps its default.
        assert_eq!(reloaded.get_i32("Count")?, -3);
        Ok(())
    }

    /// A transient setting's override is not written to disk.
    #[test]
    fn transient_settings_are_not_persisted() -> Result<(), TestError> {
        let dir = tempdir()?;
        let path = dir.join("global.toml");

        let mut store = SettingsStore::new();
        store.register("Kept", SettingValue::I32(1), "persisted")?;
        store.register_transient("Volatile", SettingValue::I32(1), "runtime only")?;
        store.set(Scope::Global, "Kept", SettingValue::I32(2))?;
        store.set(Scope::Global, "Volatile", SettingValue::I32(2))?;
        store.save_scope(Scope::Global, &path)?;

        let mut reloaded = SettingsStore::new();
        reloaded.register("Kept", SettingValue::I32(1), "persisted")?;
        reloaded.register_transient("Volatile", SettingValue::I32(1), "runtime only")?;
        assert!(reloaded.load_scope(Scope::Global, &path)?);
        assert_eq!(reloaded.get_i32("Kept")?, 2);
        // The transient override was dropped on save, so the default returns.
        assert_eq!(reloaded.get_i32("Volatile")?, 1);
        Ok(())
    }

    /// Loading a scope from an absent file is a no-op, not an error.
    #[test]
    fn loading_missing_file_is_a_no_op() -> Result<(), TestError> {
        let dir = tempdir()?;
        let mut store = populated()?;
        assert!(!store.load_scope(Scope::Global, dir.join("does-not-exist.json"))?);
        assert_eq!(store.get_i32("Count")?, -3);
        Ok(())
    }

    /// On load, an entry whose value no longer fits its declaration is dropped,
    /// while an entry for a not-yet-registered setting is kept and round-trips
    /// on the next save.
    #[test]
    fn load_drops_type_mismatches_keeps_unknowns() -> Result<(), TestError> {
        let dir = tempdir()?;
        let path = dir.join("global.toml");

        // A file written by a hypothetical other version: "Count" is now a bool
        // (type changed, no longer fits the declared i32) and "FutureSetting" is
        // not registered here yet.
        let toml = "Count = true\nFutureSetting = 42\n";
        fs_err::write(&path, toml)?;

        let mut store = populated()?;
        assert!(store.load_scope(Scope::Global, &path)?);
        // The type-changed entry was dropped, so "Count" keeps its default.
        assert_eq!(store.get_i32("Count")?, -3);
        // The unknown entry was kept and is readable generically (an integer
        // literal is inferred as an i32).
        assert_eq!(store.get("FutureSetting"), Some(&SettingValue::I32(42)));

        // It also survives a re-save (not silently discarded).
        let out = dir.join("out.toml");
        store.save_scope(Scope::Global, &out)?;
        let text = fs_err::read_to_string(&out)?;
        assert!(text.contains("FutureSetting"));
        Ok(())
    }

    /// The TOML on disk carries each setting's declared comment above a bare
    /// `name = value` line (no type tag).
    #[test]
    fn saved_toml_shape_is_stable() -> Result<(), TestError> {
        let dir = tempdir()?;
        let path = dir.join("global.toml");

        let mut store = SettingsStore::new();
        store.register("Flag", SettingValue::Bool(false), "a toggle")?;
        store.set(Scope::Global, "Flag", SettingValue::Bool(true))?;
        store.save_scope(Scope::Global, &path)?;

        let text = fs_err::read_to_string(&path)?;
        assert!(text.contains("# a toggle"), "comment missing from {text:?}");
        assert!(
            text.contains("Flag = true"),
            "value line missing from {text:?}"
        );
        Ok(())
    }

    /// Related settings are grouped under a nested `[section.subsection]` table,
    /// with each setting's declared comment above it, and round-trip cleanly.
    #[test]
    fn sections_group_and_round_trip() -> Result<(), TestError> {
        let dir = tempdir()?;
        let path = dir.join("global.toml");

        let register = |store: &mut SettingsStore| -> Result<(), SettingError> {
            store.register_in(
                &["spacenav", "flycam"],
                "FlycamAxisScale0",
                SettingValue::F32(1.0),
                "Flycam axis scaler",
            )?;
            store.register_in(
                &["spacenav", "flycam"],
                "FlycamFeathering",
                SettingValue::F32(3.0),
                "Flycam feathering",
            )
        };

        let mut store = SettingsStore::new();
        register(&mut store)?;
        store.set(Scope::Global, "FlycamAxisScale0", SettingValue::F32(1.5))?;
        store.set(Scope::Global, "FlycamFeathering", SettingValue::F32(0.5))?;
        store.save_scope(Scope::Global, &path)?;

        let text = fs_err::read_to_string(&path)?;
        assert!(
            text.contains("[spacenav.flycam]"),
            "section header missing from {text:?}"
        );
        assert!(
            text.contains("# Flycam axis scaler"),
            "comment missing from {text:?}"
        );
        // f32 values keep their short representation, not a widened f64.
        assert!(
            text.contains("FlycamAxisScale0 = 1.5"),
            "value line missing from {text:?}"
        );

        let mut reloaded = SettingsStore::new();
        register(&mut reloaded)?;
        assert!(reloaded.load_scope(Scope::Global, &path)?);
        approx(reloaded.get_f32("FlycamAxisScale0")?, 1.5);
        approx(reloaded.get_f32("FlycamFeathering")?, 0.5);
        Ok(())
    }

    /// Every value type survives a save/reload round-trip through TOML, read
    /// back through its declaration.
    #[test]
    fn all_value_types_round_trip_through_toml() -> Result<(), TestError> {
        let dir = tempdir()?;
        let path = dir.join("global.toml");

        let mut store = populated()?;
        store.set(Scope::Global, "Flag", SettingValue::Bool(false))?;
        store.set(Scope::Global, "Count", SettingValue::I32(-42))?;
        store.set(Scope::Global, "Size", SettingValue::U32(4242))?;
        // 0.1 is not exactly representable — a good precision probe.
        store.set(Scope::Global, "Clip", SettingValue::F32(0.1))?;
        store.set(
            Scope::Global,
            "Name",
            SettingValue::String("hi there".to_owned()),
        )?;
        store.set(Scope::Global, "Tint", SettingValue::Color3([0.1, 0.2, 0.3]))?;
        store.set(
            Scope::Global,
            "Glow",
            SettingValue::Color4([0.1, 0.2, 0.3, 0.4]),
        )?;
        store.set(Scope::Global, "Offset", SettingValue::Vec3([1.5, 2.5, 3.5]))?;
        store.set(
            Scope::Global,
            "Region",
            SettingValue::Vec3d([4.0, 5.0, 6.0]),
        )?;
        store.set(
            Scope::Global,
            "Window",
            SettingValue::Rect([0, 100, 200, 0]),
        )?;
        store.save_scope(Scope::Global, &path)?;

        // The precision probe writes the short f32 form, not a widened f64.
        let text = fs_err::read_to_string(&path)?;
        assert!(
            text.contains("Clip = 0.1"),
            "f32 not short-formatted in {text:?}"
        );

        let mut reloaded = populated()?;
        assert!(reloaded.load_scope(Scope::Global, &path)?);
        assert!(!reloaded.get_bool("Flag")?);
        assert_eq!(reloaded.get_i32("Count")?, -42);
        assert_eq!(reloaded.get_u32("Size")?, 4242);
        approx(reloaded.get_f32("Clip")?, 0.1);
        assert_eq!(reloaded.get_str("Name")?, "hi there");
        approx_slice(&reloaded.get_color3("Tint")?, &[0.1, 0.2, 0.3]);
        approx_slice(&reloaded.get_color4("Glow")?, &[0.1, 0.2, 0.3, 0.4]);
        approx_slice(&reloaded.get_vec3("Offset")?, &[1.5, 2.5, 3.5]);
        approx_slice_f64(&reloaded.get_vec3d("Region")?, &[4.0, 5.0, 6.0]);
        assert_eq!(reloaded.get_rect("Window")?, [0, 100, 200, 0]);
        Ok(())
    }

    /// Create a unique temporary directory for a persistence test, namespaced by
    /// crate + test so parallel `nextest` binaries never share a path.
    fn tempdir() -> Result<std::path::PathBuf, TestError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "{}-settings-{nanos}-{:?}",
            env!("CARGO_PKG_NAME"),
            std::thread::current().id()
        ));
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }
}
