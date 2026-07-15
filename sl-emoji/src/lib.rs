//! An emoji dataset and lookup layer for the Second Life / OpenSim viewer.
//!
//! The viewer needs emoji in two places: an inline `:`-completer that turns a
//! typed short-code (`:rocket:`) into a glyph, and a picker floater that shows a
//! grouped, searchable grid. Both are backed by the same data, exposed here
//! behind a small, UI-free API.
//!
//! Rather than hand-author the data — the colon **short-codes** are the de-facto
//! [gemoji] convention and the canonical **names / groups** are Unicode CLDR
//! annotations — this crate wraps the [`emojis`] crate, which bundles both
//! (gemoji short-codes over Unicode-CLDR-ordered emoji, with names and groups)
//! as a self-contained, no-network dependency. The wrapper insulates the
//! viewer from that crate's exact API and narrows it to what the two consumers
//! need.
//!
//! The reference viewer's counterpart is `llemojidictionary` (the lookup) feeding
//! `llpanelemojicomplete` (the inline completer) and `llfloateremojipicker`
//! (the picker).
//!
//! # What the data covers
//!
//! - **Short-code ↔ glyph** — [`by_shortcode`] / [`by_glyph`] resolve exact
//!   codes and glyphs; [`complete`] prefix-completes a short-code for the
//!   inline completer.
//! - **Grouped, searchable list** — [`Group`] partitions the dataset into the
//!   picker's tabs ([`Group::emojis`]); [`search`] ranks a free-text query
//!   across names and short-codes.
//! - **Skin tones** — the six single [`SkinTone`]s a picker offers, via
//!   [`Emoji::with_skin_tone`].
//!
//! What it deliberately does **not** carry: the backing crate exposes CLDR
//! *names* and gemoji *short-codes* but no separate CLDR *keyword* list, so
//! [`search`] matches names and short-codes only. If richer keyword search is
//! wanted later, pull CLDR annotations (via `icu4x`) behind this same API.
//!
//! # Example
//!
//! ```
//! # use sl_emoji::{by_shortcode, complete, search, Group, SkinTone};
//! // Short-code ↔ glyph, colons and case ignored.
//! let rocket = by_shortcode(":Rocket:").unwrap();
//! assert_eq!(rocket.glyph(), "🚀");
//! assert_eq!(rocket.name(), "rocket");
//!
//! // The inline completer: a prefix expands to its short-codes.
//! assert!(complete("rock").iter().any(|m| m.shortcode == "rocket"));
//!
//! // The picker's search box ranks an exact short-code first.
//! assert_eq!(search("rocket").first().map(|e| e.glyph()), Some("🚀"));
//!
//! // The picker's tabs, and skin tones for a person emoji.
//! assert_eq!(Group::ALL.len(), 9);
//! let wave = by_shortcode("wave").unwrap();
//! assert!(wave.with_skin_tone(SkinTone::Dark).is_some());
//! ```
//!
//! [gemoji]: https://github.com/github/gemoji

pub mod emoji;
pub mod group;
pub mod lookup;

pub use emoji::{Emoji, SkinTone};
pub use group::Group;
pub use lookup::{ShortcodeMatch, all, by_glyph, by_shortcode, complete, search};

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::{Group, SkinTone, all, by_glyph, by_shortcode, complete, search};

    /// A boxed error so tests can use `?` on `Result` and `Option` instead of
    /// the disallowed `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A short-code resolves to its glyph, with surrounding colons and case
    /// ignored, and the glyph resolves back.
    #[test]
    fn shortcode_and_glyph_round_trip() -> Result<(), TestError> {
        let rocket = by_shortcode(":Rocket:").ok_or("no :rocket:")?;
        assert_eq!(rocket.glyph(), "🚀");
        assert_eq!(rocket.name(), "rocket");
        assert_eq!(rocket.shortcode(), Some("rocket"));
        assert_eq!(by_glyph("🚀"), Some(rocket));
        Ok(())
    }

    /// An unknown short-code or glyph is a clean miss, not a panic.
    #[test]
    fn unknown_lookups_return_none() {
        assert_eq!(by_shortcode("definitely_not_an_emoji"), None);
        assert_eq!(by_glyph("not an emoji"), None);
        assert_eq!(by_glyph("🚀🚀"), None);
    }

    /// An emoji with several gemoji short-codes exposes all of them.
    #[test]
    fn multiple_shortcodes_are_all_present() -> Result<(), TestError> {
        let laughing = by_glyph("😆").ok_or("no 😆")?;
        let codes: Vec<&str> = laughing.shortcodes().collect();
        assert!(codes.contains(&"laughing"), "codes: {codes:?}");
        assert!(codes.contains(&"satisfied"), "codes: {codes:?}");
        Ok(())
    }

    /// Completion returns every short-code with the prefix, sorted, each
    /// resolving to a real emoji.
    #[test]
    fn complete_prefix_is_sorted_and_matches() {
        let matches = complete(":rock");
        assert!(!matches.is_empty());
        assert!(matches.iter().any(|m| m.shortcode == "rocket"));
        // Every hit actually starts with the prefix...
        assert!(matches.iter().all(|m| m.shortcode.starts_with("rock")));
        // ...and the list is sorted by short-code.
        let mut sorted = matches.clone();
        sorted.sort_by(|a, b| a.shortcode.cmp(b.shortcode));
        assert_eq!(matches, sorted);
        // The carried emoji matches its short-code.
        for hit in &matches {
            assert_eq!(by_shortcode(hit.shortcode), Some(hit.emoji));
        }
    }

    /// An empty (or colon-only) completion prefix returns nothing rather than
    /// the whole dataset.
    #[test]
    fn complete_empty_prefix_is_empty() {
        assert!(complete("").is_empty());
        assert!(complete(":").is_empty());
    }

    /// Search ranks an exact short-code first, then looser matches, and covers
    /// name matches too.
    #[test]
    fn search_ranks_exact_shortcode_first() -> Result<(), TestError> {
        // Exact short-code beats every substring hit.
        assert_eq!(search("rocket").first().map(|e| e.glyph()), Some("🚀"));
        // A name-based query still finds the glyph.
        let grin = by_glyph("😀").ok_or("no 😀")?;
        assert!(search("grinning").contains(&grin));
        // Case-insensitive.
        assert_eq!(search("ROCKET").first().map(|e| e.glyph()), Some("🚀"));
        Ok(())
    }

    /// A blank search returns nothing (the picker shows its grouped list).
    #[test]
    fn search_blank_query_is_empty() {
        assert!(search("").is_empty());
        assert!(search("   ").is_empty());
    }

    /// Every group has a title and a non-empty emoji list, and each listed
    /// emoji reports the group it was listed under.
    #[test]
    fn groups_partition_the_dataset() {
        assert_eq!(Group::ALL.len(), 9);
        for group in Group::ALL {
            assert!(!group.title().is_empty());
            let mut count = 0_usize;
            for emoji in group.emojis() {
                assert_eq!(emoji.group(), group);
                count = count.saturating_add(1);
            }
            assert!(count > 0, "{group:?} was empty");
        }
    }

    /// The whole dataset iterates, and it is the union of the groups.
    #[test]
    fn all_is_the_union_of_groups() {
        let total = all().count();
        let by_group: usize = Group::ALL
            .into_iter()
            .map(|g| g.emojis().count())
            .fold(0_usize, usize::saturating_add);
        assert_eq!(total, by_group);
        assert!(total > 1000, "unexpectedly small dataset: {total}");
    }

    /// A person emoji takes skin tones and each tone renders a distinct glyph;
    /// a non-person emoji takes none.
    #[test]
    fn skin_tones_apply_to_people_only() -> Result<(), TestError> {
        let wave = by_shortcode("wave").ok_or("no :wave:")?;
        assert!(wave.has_skin_tones());
        let dark = wave.with_skin_tone(SkinTone::Dark).ok_or("no dark wave")?;
        assert_ne!(dark.glyph(), wave.glyph());
        // The default tone is the plain glyph.
        assert_eq!(
            wave.with_skin_tone(SkinTone::Default).map(|e| e.glyph()),
            Some(wave.glyph())
        );

        let rocket = by_glyph("🚀").ok_or("no 🚀")?;
        assert!(!rocket.has_skin_tones());
        assert_eq!(rocket.with_skin_tone(SkinTone::Dark), None);
        assert_eq!(rocket.skin_tone(), None);
        Ok(())
    }

    /// The six offered skin tones are exactly `SkinTone::ALL`.
    #[test]
    fn skin_tone_all_is_the_six_single_tones() {
        assert_eq!(SkinTone::ALL.len(), 6);
        assert_eq!(SkinTone::ALL.first(), Some(&SkinTone::Default));
    }

    /// Unicode version metadata is exposed (rocket predates emoji 1.0's
    /// versioning but is a well-known early glyph).
    #[test]
    fn unicode_version_is_reported() -> Result<(), TestError> {
        let hand = by_glyph("🤌").ok_or("no 🤌")?;
        assert_eq!(hand.unicode_version(), (13, 0));
        Ok(())
    }
}
