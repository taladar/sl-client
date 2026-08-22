//! Every key the notification catalogue names must exist in the shipped
//! English Fluent bundle.
//!
//! This check spans two crates by nature: the catalogue is
//! `sl-viewer-notifications`, the `.ftl` bundles are shipped assets of the
//! viewer. It lives here because the viewer owns the assets — putting it with
//! the catalogue would mean that crate reaching across a directory boundary
//! with `include_str!`, which widens its commit-hook relevance to the whole
//! repository (see `book/src/tools/build-performance.md`).
//!
//! A missing key is not a compile error and not a panic — it renders as a raw
//! Fluent identifier in front of the user — so it needs a test to catch it.

#[cfg(test)]
mod test {
    use std::collections::HashSet;

    use sl_viewer_notifications::NOTIFICATIONS;

    /// The English Fluent bundle source, embedded so the catalogue's keys can be
    /// checked against it without the async asset load.
    const EN_FTL: &str = include_str!("../assets/locales/en/main.ftl");

    /// The set of message identifiers declared in [`EN_FTL`] — a message entry
    /// begins at column 0 with `identifier =` (attributes and continuation lines
    /// are indented, comments begin with `#`).
    fn ftl_keys() -> HashSet<String> {
        EN_FTL
            .lines()
            .filter_map(|line| {
                if line.starts_with([' ', '\t', '#']) {
                    return None;
                }
                let ident = line.split_once('=')?.0.trim();
                if ident.is_empty() || ident.contains(char::is_whitespace) {
                    return None;
                }
                Some(ident.to_owned())
            })
            .collect()
    }

    /// Every message, button label, input default, title and ignore label in the
    /// catalogue resolves against the English bundle.
    #[test]
    fn every_key_has_an_english_fluent_entry() {
        let keys = ftl_keys();
        for entry in NOTIFICATIONS {
            assert!(
                keys.contains(entry.message_key),
                "{}: message_key {} has no en/main.ftl entry",
                entry.name,
                entry.message_key
            );
            for button in entry.form {
                assert!(
                    keys.contains(button.label_key),
                    "{}: button label_key {} has no en/main.ftl entry",
                    entry.name,
                    button.label_key
                );
            }
            if let Some(key) = entry.input.and_then(|input| input.default_key) {
                assert!(
                    keys.contains(key),
                    "{}: input default_key {} has no en/main.ftl entry",
                    entry.name,
                    key
                );
            }
            if let Some(key) = entry.title_key {
                assert!(
                    keys.contains(key),
                    "{}: title_key {} has no en/main.ftl entry",
                    entry.name,
                    key
                );
            }
            if let Some(key) = entry.ignore_key {
                assert!(
                    keys.contains(key),
                    "{}: ignore_key {} has no en/main.ftl entry",
                    entry.name,
                    key
                );
            }
        }
    }
}
