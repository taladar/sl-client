//! Holds the English bundle and the source that names it to each other.
//!
//! A translated string is looked up by **key**, and a key the bundle does not
//! define resolves to *itself* — the Fluent convention, and the right one, since
//! a visible `menu-bar-mini-mpa` beats a blank menu line. What it is not is
//! loud: the viewer goes on working, the wrong string is drawn only where
//! someone happens to look, and a `warn!` in the journal
//! (`i18n::report_missing_key`) is seen only by whoever is reading it. So the
//! two directions are checked here instead, against the *source*:
//!
//! - a key the source names and the bundle does not define would ship as its
//!   own name drawn on a panel;
//! - a key the bundle defines and no source names is a string every translator
//!   goes on translating for a feature that no longer exists.
//!
//! # Why the source and not the running viewer
//!
//! The obvious version — stand a viewer up, walk what it registered, ask the
//! bundle — reaches less than it looks. Barely half the settings are declared
//! by [`crate::REGISTRARS`]; the rest are registered by a *system* on first use
//! (the radar's alert toggles, a floater's remembered rectangle), so an app that
//! has not opened that floater never declares that key. The menus are worse: a
//! domain's entry trees are `static`s private to their own crate, so no test
//! anywhere can enumerate them. The declarations are all in the text, though,
//! and in a handful of fixed spellings — which is what this reads.
//!
//! Test-only fixtures are skipped: a menu fixture inside a test module names
//! keys the shipped bundle has no business carrying, and the harness those
//! fixtures run under resolves every key to itself anyway
//! (`sl_viewer_ui_core::i18n::install_untranslated`).

/// The viewer's shipped English bundle, as text — the base every other locale
/// falls back to, so a key missing *here* is missing everywhere.
///
/// Embedded **in this crate** rather than in `sl-viewer-testkit`, which is where
/// the harnesses that want it live: the bundle is a shipped asset of the viewer,
/// and an `include_str!` of it from a crate the whole viewer dev-depends on
/// would make every edit to a string re-run every test above that crate. The
/// same reasoning puts `tests/notification_ftl_coverage.rs` here; see
/// `book/src/tools/build-performance.md`.
#[cfg(test)]
pub(crate) const ENGLISH_BUNDLE: &str = include_str!("../assets/locales/en/main.ftl");

/// Install the string half of a harness with the **real English strings**
/// behind it — `sl_viewer_ui_core::i18n::install_english` over
/// [`ENGLISH_BUNDLE`].
///
/// What a harness wants when it **measures** a layout: a compass label is `N`
/// and a pie slice reads `Take`, where the key-answers-itself harness
/// (`install_untranslated`) would draw `trackball-north` into a box sized for
/// one letter. A test naming the keys a line is built from wants that other
/// one — or [`english`], which resolves a key the same way this does.
#[cfg(test)]
pub(crate) fn install_english_strings(app: &mut bevy::prelude::App) {
    sl_viewer_ui_core::i18n::install_english(app, ENGLISH_BUNDLE);
}

/// The text [`ENGLISH_BUNDLE`] gives `key`, or the key itself if it defines no
/// argument-free message under that name.
///
/// For a test that asserts **which strings a line is built from** while the
/// fold resolves the real English: comparing a drawn line against
/// `english("build-selection-none")` says the line is built from that key
/// without pinning its wording, which is the shipped bundle's business. A line
/// built from the wrong key still fails, because the two keys' texts differ.
#[cfg(test)]
pub(crate) fn english(key: &str) -> String {
    /// Parsed once: a test may ask for many keys, and the bundle is thousands
    /// of lines.
    static TABLE: std::sync::LazyLock<sl_viewer_ui_core::i18n::BundlelessStrings> =
        std::sync::LazyLock::new(|| {
            sl_viewer_ui_core::i18n::BundlelessStrings::from_ftl(ENGLISH_BUNDLE)
        });
    TABLE.get(key).unwrap_or(key).to_owned()
}

#[cfg(test)]
mod test {
    use std::collections::{BTreeSet, HashSet};

    use super::ENGLISH_BUNDLE as BUNDLE;

    /// A boxed error so tests use `?` rather than disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The key prefixes this checks. Each names a family whose members are
    /// declared as plain literals in the source, so both directions can be
    /// answered by reading it.
    const CHECKED_PREFIXES: &[&str] = &["setting-desc-", "menu-", "pie-"];

    /// Every message key the English bundle defines.
    ///
    /// A line-wise read rather than a Fluent parse: the question is "is this
    /// name on the left of an `=` at column 0", and a test that loaded the
    /// bundle through `bevy_fluent` would need an asset server and a frame loop
    /// to answer it. Continuation lines of a multi-line value are indented, and
    /// a selector's variants start with `[` or `*`, so neither is mistaken for
    /// a key.
    fn defined_keys() -> HashSet<&'static str> {
        BUNDLE
            .lines()
            .filter_map(|line| line.split_once(" ="))
            .map(|(key, _value)| key)
            .filter(|key| {
                !key.is_empty()
                    && key
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            })
            .collect()
    }

    /// Every `.rs` file of the viewer and the `sl-viewer-*` crates, with
    /// anything from its test module on cut off.
    ///
    /// Read at run time rather than `include!`d, exactly as
    /// `settings_golden`'s registrar sweep is, so this crate's commit-hook
    /// relevance does not widen to the whole repository.
    fn shipped_sources() -> Result<Vec<String>, TestError> {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest
            .parent()
            .ok_or_else(|| -> TestError { "no workspace root".into() })?;
        let mut roots = vec![manifest.join("src")];
        for entry in fs_err::read_dir(workspace)? {
            let path = entry?.path();
            let is_viewer_crate = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("sl-viewer-") || name == "sl-settings");
            if is_viewer_crate && path.join("src").is_dir() {
                roots.push(path.join("src"));
            }
        }
        let mut sources = Vec::new();
        while let Some(dir) = roots.pop() {
            for entry in fs_err::read_dir(&dir)? {
                let path = entry?.path();
                if path.is_dir() {
                    roots.push(path);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                let body = fs_err::read_to_string(&path)?;
                // Both spellings the workspace uses for a test module, since
                // this very file is one of them and its marker strings would
                // otherwise read as keys nothing defines.
                let cut = ["\nmod tests {", "\nmod test {"]
                    .iter()
                    .filter_map(|marker| body.find(marker))
                    .min();
                let shipped = cut.map_or(body.as_str(), |at| body.get(..at).unwrap_or(&body));
                sources.push(shipped.to_owned());
            }
        }
        Ok(sources)
    }

    /// The string literal that follows `from` in `source` once whitespace is
    /// skipped, or `None` if what follows is not one.
    ///
    /// The whitespace skip is what makes this survive `rustfmt`: a call whose
    /// arguments do not fit one line puts its key on the next, which a
    /// fixed-text marker would walk straight past — and a key nothing appears
    /// to name reads to the orphan check as a string to delete.
    fn literal_after(source: &str, from: usize) -> Option<&str> {
        let rest = source.get(from..)?.trim_start();
        let body = rest.strip_prefix('"')?;
        let end = body.find('"')?;
        body.get(..end)
    }

    /// Every occurrence of `marker` in `source`, as the byte just past it.
    ///
    /// Saturating throughout, per the workspace's `arithmetic_side_effects`
    /// lint: the sums are byte offsets into a string that is already in memory,
    /// so they cannot overflow in practice, and saturating says so without a
    /// suppression.
    fn occurrences<'a>(source: &'a str, marker: &'a str) -> impl Iterator<Item = usize> + 'a {
        let mut at = 0_usize;
        core::iter::from_fn(move || {
            let hit = source.get(at..)?.find(marker)?;
            let past = at.saturating_add(hit).saturating_add(marker.len());
            at = past;
            Some(past)
        })
    }

    /// The declaration spellings whose next argument is a Fluent key: a menu or
    /// pie label, a line-menu command, and the two menu-bar helpers that take a
    /// label ahead of a setting name.
    const KEY_BEARING_CALLS: &[&str] = &[
        "label_key:",
        "MenuCommand::new(",
        "rlv_toggle(",
        "rlv_master_toggle(",
    ];

    /// Every Fluent key the shipped source names: a settings description (whose
    /// `"setting-desc-` prefix is unambiguous on its own) and the first argument
    /// of each [`KEY_BEARING_CALLS`] spelling.
    ///
    /// Deliberately *not* "every literal that looks like a key": `"menu-bar"`
    /// is an element name and `"top-menu-bar"` an action tag, neither of which
    /// the bundle has any reason to define.
    fn named_keys(sources: &[String]) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        for source in sources {
            for past in occurrences(source, "\"setting-desc-") {
                let start = past.saturating_sub("setting-desc-".len());
                if let Some(key) = source
                    .get(start..)
                    .and_then(|rest| rest.find('"').and_then(|end| rest.get(..end)))
                {
                    found.insert(key.to_owned());
                }
            }
            for call in KEY_BEARING_CALLS {
                for past in occurrences(source, call) {
                    if let Some(key) = literal_after(source, past) {
                        found.insert(key.to_owned());
                    }
                }
            }
        }
        found
    }

    /// **Every key the source names is defined in the English bundle.**
    ///
    /// The failure this catches is a typo in a key: nothing breaks, and the
    /// misspelt key is simply drawn where the words should have been.
    #[test]
    fn every_key_the_source_names_is_defined() -> Result<(), TestError> {
        let defined = defined_keys();
        let missing: Vec<String> = named_keys(&shipped_sources()?)
            .into_iter()
            .filter(|key| !defined.contains(key.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "these keys are named in the source but defined by no English \
             string, so each would be drawn as its own name: {missing:#?}"
        );
        Ok(())
    }

    /// **Every key the bundle defines under a checked prefix is named by the
    /// source.**
    ///
    /// The failure this catches is the other half of a deletion: an entry or a
    /// setting goes, its string stays, and every translator goes on translating
    /// a line nothing draws.
    #[test]
    fn every_key_the_bundle_defines_is_named() -> Result<(), TestError> {
        let named = named_keys(&shipped_sources()?);
        let orphans: Vec<&str> = defined_keys()
            .into_iter()
            .filter(|key| {
                CHECKED_PREFIXES
                    .iter()
                    .any(|prefix| key.starts_with(prefix))
            })
            .filter(|key| !named.contains(*key))
            .collect();
        assert!(
            orphans.is_empty(),
            "the English bundle defines these, and nothing in the viewer asks \
             for them any more — delete them: {orphans:#?}"
        );
        Ok(())
    }
}
