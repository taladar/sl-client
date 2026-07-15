//! The lookups the viewer's emoji features are built on: exact
//! short-code / glyph resolution, prefix completion for the inline
//! `:`-completer, and a ranked search for the picker.

use crate::emoji::Emoji;

/// The emoji that some gemoji short-code prefix completes to.
///
/// A single emoji can appear under more than one short-code (e.g. `"laughing"`
/// and `"satisfied"` both map to `😆`), so completion yields one entry *per
/// matching short-code*, carrying the short-code that matched so the completer
/// can display and insert it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcodeMatch {
    /// The matched short-code, without the surrounding colons.
    pub shortcode: &'static str,
    /// The emoji it resolves to.
    pub emoji: Emoji,
}

/// Look an emoji up by its exact glyph, e.g. `by_glyph("🚀")`. Returns [`None`]
/// for a string that is not exactly one dataset emoji.
#[must_use]
pub fn by_glyph(glyph: &str) -> Option<Emoji> {
    emojis::get(glyph).map(Emoji::new)
}

/// Look an emoji up by a gemoji short-code, e.g. `by_shortcode("rocket")` or
/// `by_shortcode(":rocket:")`. Surrounding colons are ignored and matching is
/// case-insensitive. Returns [`None`] when no short-code matches.
#[must_use]
pub fn by_shortcode(shortcode: &str) -> Option<Emoji> {
    let code = shortcode.trim_matches(':').to_ascii_lowercase();
    emojis::get_by_shortcode(&code).map(Emoji::new)
}

/// Every emoji in the dataset, in canonical CLDR order and default skin tone
/// only (skin-tone variants are reached via [`Emoji::with_skin_tone`]).
pub fn all() -> impl Iterator<Item = Emoji> + Clone {
    emojis::iter().map(Emoji::new)
}

/// Complete a gemoji short-code from a prefix, for the inline `:`-completer:
/// every short-code that starts with `prefix` (colons and case ignored),
/// sorted alphabetically by short-code.
///
/// An empty prefix returns nothing — the completer opens on a real prefix
/// rather than listing the whole dataset.
#[must_use]
pub fn complete(prefix: &str) -> Vec<ShortcodeMatch> {
    let needle = prefix.trim_start_matches(':').to_ascii_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<ShortcodeMatch> = emojis::iter()
        .flat_map(|backing| {
            backing
                .shortcodes()
                .filter(|code| code.starts_with(needle.as_str()))
                .map(move |shortcode| ShortcodeMatch {
                    shortcode,
                    emoji: Emoji::new(backing),
                })
        })
        .collect();
    matches.sort_by(|a, b| a.shortcode.cmp(b.shortcode));
    matches
}

/// How well an emoji matched a search query. Ordered best (smallest) first, so
/// a `sort` on it ranks the results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchRank {
    /// A short-code equals the query exactly.
    ExactShortcode,
    /// A short-code starts with the query.
    ShortcodePrefix,
    /// A word of the CLDR name starts with the query.
    NameWordPrefix,
    /// The CLDR name contains the query somewhere.
    NameContains,
    /// A short-code contains the query somewhere.
    ShortcodeContains,
}

/// The best (smallest) rank at which `backing` matches the already-lowercased
/// `needle`, or [`None`] if it does not match at all.
fn rank(backing: &emojis::Emoji, needle: &str) -> Option<MatchRank> {
    /// Fold a candidate rank into the running best (smaller wins).
    fn consider(rank: MatchRank, best: &mut Option<MatchRank>) {
        if best.is_none_or(|current| rank < current) {
            *best = Some(rank);
        }
    }

    let mut best: Option<MatchRank> = None;
    for code in backing.shortcodes() {
        if code == needle {
            consider(MatchRank::ExactShortcode, &mut best);
        } else if code.starts_with(needle) {
            consider(MatchRank::ShortcodePrefix, &mut best);
        } else if code.contains(needle) {
            consider(MatchRank::ShortcodeContains, &mut best);
        }
    }

    let name = backing.name().to_ascii_lowercase();
    if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| !word.is_empty() && word.starts_with(needle))
    {
        consider(MatchRank::NameWordPrefix, &mut best);
    } else if name.contains(needle) {
        consider(MatchRank::NameContains, &mut best);
    }

    best
}

/// Search the dataset for `query`, matching against CLDR names and gemoji
/// short-codes (case-insensitive), for the picker's search box.
///
/// Results are ranked: an exact short-code first, then short-code and
/// name-word prefixes, then loose substring matches, with CLDR order breaking
/// ties. A blank query returns nothing — the picker shows its grouped list
/// (via [`Group::emojis`](crate::Group::emojis)) rather than a search result.
#[must_use]
pub fn search(query: &str) -> Vec<Emoji> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(MatchRank, usize, Emoji)> = emojis::iter()
        .enumerate()
        .filter_map(|(index, backing)| {
            rank(backing, &needle).map(|rank| (rank, index, Emoji::new(backing)))
        })
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    ranked.into_iter().map(|(_, _, emoji)| emoji).collect()
}
