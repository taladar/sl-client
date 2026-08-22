//! Pins the full set of settings the viewer declares.
//!
//! The settings store cannot name its own users — that is why the registrar
//! list lives in the binary crate ([`crate::REGISTRARS`]) rather than beside
//! the store. The cost of that inversion is that dropping a registrar is
//! *silent*: registration merely stops happening, `SettingsStore::get` then
//! reports the name as unregistered, and the caller falls back to a default.
//! For the login start location that means a user's saved "log in at my last
//! position" quietly reverts to the home point, with nothing logged.
//!
//! So the declared surface is checked in, and this test compares against it.
//! It covers the default value too, since a changed default is the same class
//! of silent behaviour change as a missing registration.
//!
//! When a setting is deliberately added, removed or re-defaulted, regenerate
//! the golden file with:
//!
//! ```console
//! SL_VIEWER_BLESS_SETTINGS_GOLDEN=1 cargo test -p sl-client-bevy-viewer settings_golden
//! ```

#[cfg(test)]
mod test {
    use crate::REGISTRARS;
    use crate::settings::ViewerSettings;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests use `?` rather than disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The checked-in surface: one `section/name = kind default [flags]` line
    /// per declared setting, sorted by name.
    const GOLDEN: &str = include_str!("../tests/settings-golden.txt");

    /// Render the declared surface of a store as sorted, diffable lines.
    fn declared_surface() -> String {
        let settings = ViewerSettings::declared_for_test(REGISTRARS);
        let store = settings.store();
        let mut lines: Vec<String> = store
            .names()
            .filter_map(|name| {
                let decl = store.declaration(name)?;
                let section = decl.section().join(".");
                let mut flags = Vec::new();
                if !decl.persist() {
                    flags.push("transient");
                }
                if decl.editor_hidden() {
                    flags.push("editor-hidden");
                }
                let suffix = if flags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", flags.join(","))
                };
                Some(format!(
                    "{section}/{name} = {:?} {:?}{suffix}",
                    decl.kind(),
                    decl.default()
                ))
            })
            .collect();
        lines.sort();
        let mut rendered = lines.join("\n");
        rendered.push('\n');
        rendered
    }

    /// Every setting the viewer declares, and its default, is the set recorded
    /// in `tests/settings-golden.txt`.
    #[test]
    fn declared_settings_match_the_golden_file() -> Result<(), TestError> {
        let actual = declared_surface();
        if std::env::var_os("SL_VIEWER_BLESS_SETTINGS_GOLDEN").is_some() {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/settings-golden.txt");
            fs_err::write(path, &actual)?;
            return Ok(());
        }
        assert_eq!(
            actual, GOLDEN,
            "the declared settings surface changed; if deliberate, re-bless with \
             SL_VIEWER_BLESS_SETTINGS_GOLDEN=1"
        );
        Ok(())
    }

    /// Every module that defines a `register_settings` is listed in
    /// [`REGISTRARS`].
    ///
    /// The golden file cannot catch an omission on its own: if a registrar is
    /// missing when the golden is generated, the missing settings are simply
    /// absent from both sides and it passes. That is not hypothetical — the
    /// commit that introduced `REGISTRARS` dropped `i18n` (an extraction regex
    /// that did not allow digits in a module name), and only the `dead_code`
    /// lint on the now-uncalled function caught it. This test compares the
    /// source against the list directly, so the two cannot agree by both being
    /// wrong.
    #[test]
    fn every_module_defining_a_registrar_is_listed() -> Result<(), TestError> {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let listed = include_str!("lib.rs");
        let mut missing = Vec::new();
        // The viewer's own modules, plus the extracted `sl-viewer-*` crates —
        // a registrar keeps working after its module moves out (the alias
        // preserves the path), so the search has to follow it. Read at run
        // time rather than `include!`d, which would widen this crate's
        // commit-hook relevance to the whole repository.
        let mut roots = vec![manifest.join("src")];
        let workspace = manifest
            .parent()
            .ok_or_else(|| -> TestError { "no workspace root".into() })?;
        for entry in fs_err::read_dir(workspace)? {
            let path = entry?.path();
            let is_viewer_crate = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("sl-viewer-"));
            if is_viewer_crate && path.join("src").is_dir() {
                roots.push(path.join("src"));
            }
        }
        for src in roots {
            for entry in fs_err::read_dir(&src)? {
                let path = entry?.path();
                if path.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let body = fs_err::read_to_string(&path)?;
                // A definition at column 0 with a visibility modifier is what
                // takes part in the aggregation protocol. `volume_panel` and
                // `world_sounds` each have a *private* helper of the same name
                // that they call themselves, which is not the same thing; and
                // requiring column 0 keeps this file's own mention of the pattern
                // (inside a string literal, indented) from matching itself.
                let defines = body.lines().any(|line| {
                    line.starts_with("pub(crate) fn register_settings(")
                        || line.starts_with("pub fn register_settings(")
                });
                if defines && !listed.contains(&format!("crate::{stem}::register_settings")) {
                    missing.push(stem.to_owned());
                }
            }
        }
        missing.sort();
        assert!(
            missing.is_empty(),
            "these modules define `register_settings` but are absent from REGISTRARS, \
             so their settings are silently never declared: {missing:?}"
        );
        Ok(())
    }

    /// Every registrar contributes at least one setting — a registrar that
    /// declares nothing is either dead or silently broken.
    #[test]
    fn every_registrar_declares_something() {
        for (index, register) in REGISTRARS.iter().enumerate() {
            let mut settings = ViewerSettings::declared_for_test(&[]);
            settings.run_registrars(&[*register]);
            assert!(
                settings.store().names().next().is_some(),
                "REGISTRARS[{index}] declared no settings"
            );
        }
    }
}
