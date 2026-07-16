//! Pseudolocalisation: a machine-generated stand-in for a translation, so the
//! whole UI can be tested against "a language that is not English" before a
//! single translator has been involved.
//!
//! # Why this is not a toy
//!
//! The most common way a UI breaks in a language its author never saw is the
//! dullest: the string got longer and something that was measured once, in
//! English, no longer fits. German and Finnish routinely run 30–40% longer than
//! English; CJK runs shorter but taller. Waiting for real `.ftl` bundles
//! ([`viewer-i18n-fluent-scaffold`]) to find that out means finding it out late,
//! in a language the author cannot read, in a panel that shipped.
//!
//! [`pseudolocalise`] fakes the hard parts of that on demand:
//!
//! - **Expansion.** Pad to ~140% of the original, which is the length the long
//!   European languages actually reach. A layout that survives this survives
//!   German.
//! - **Accents.** Map each ASCII letter to a look-alike carrying a diacritic.
//!   The text stays readable to an English speaker (so a human can still use the
//!   gallery), but it leaves the ASCII range — which catches a font that has no
//!   glyph beyond ASCII, and any code that assumed one byte per character.
//! - **Brackets.** Fence the string in `⟦…⟧`. This is the load-bearing one: if
//!   either bracket is missing on screen, the string was **truncated**, and
//!   truncation is otherwise nearly invisible in a language you cannot read.
//!
//! Deliberately *not* done: reordering, or faking RTL by reversing the string.
//! Direction is a real axis of the matrix driven by [`crate::ui::UiDirection`]
//! and real RTL samples, and a fake would only prove the fake works.
//!
//! # Who owns this later
//!
//! [`viewer-i18n-fluent-scaffold`] takes it over as a pseudo-*locale* — the
//! transform applied at the Fluent lookup, so every string in the UI turns
//! pseudo at once from one switch, rather than each call site opting in. The
//! transform itself is this function either way, which is why it lives in its
//! own module rather than inside the harness or the gallery: both already use
//! it, and i18n will too.
//!
//! [`viewer-i18n-fluent-scaffold`]: ../../../roadmap/ready/viewer-i18n-fluent-scaffold.md

/// The opening fence. If this is not on screen, the string was clipped at the
/// start.
const OPEN: char = '⟦';

/// The closing fence. If this is not on screen, the string was clipped at the
/// end — the common case, and the one this exists to make obvious.
const CLOSE: char = '⟧';

/// The character padded onto the end to reach the expanded length. A word-joiner
/// would be invisible; a letter would read as part of the text. A middle dot is
/// unmistakably filler and still occupies a real advance width.
const PAD: char = '·';

/// How many filler characters go in a run before a space breaks it.
///
/// The filler has to offer wrap opportunities: an unbroken run is one token, and
/// no line can break inside it. Short enough to wrap freely, long enough to still
/// read as filler rather than as a dotted line.
const PAD_RUN: usize = 3;

/// The percentage of the original length the result is padded out to.
///
/// 140% is not a round number chosen for looks: it is roughly where German and
/// Finnish land against English, so a panel that holds a pseudolocalised string
/// holds a real translation.
const EXPANSION_PERCENT: usize = 140;

/// Map one ASCII letter to a look-alike carrying a diacritic.
///
/// Non-ASCII is returned unchanged, which is the right answer rather than a
/// shortcut: a CJK or Arabic sample has nothing to accent, and mangling it would
/// only test the mangling. Characters outside the letter range (spaces,
/// punctuation, digits) pass through so the text keeps its shape and stays
/// legible.
const fn accent(character: char) -> char {
    match character {
        'a' => 'á',
        'b' => 'ƀ',
        'c' => 'ç',
        'd' => 'ð',
        'e' => 'é',
        'f' => 'ƒ',
        'g' => 'ğ',
        'h' => 'ĥ',
        'i' => 'í',
        'j' => 'ĵ',
        'k' => 'ķ',
        'l' => 'ł',
        'm' => 'ɱ',
        'n' => 'ñ',
        'o' => 'ó',
        'p' => 'þ',
        'q' => 'ƣ',
        'r' => 'ŕ',
        's' => 'š',
        't' => 'ţ',
        'u' => 'ú',
        'v' => 'ṽ',
        'w' => 'ŵ',
        'x' => 'ẋ',
        'y' => 'ý',
        'z' => 'ž',
        'A' => 'Á',
        'B' => 'Ɓ',
        'C' => 'Ç',
        'D' => 'Ð',
        'E' => 'É',
        'F' => 'Ƒ',
        'G' => 'Ğ',
        'H' => 'Ĥ',
        'I' => 'Í',
        'J' => 'Ĵ',
        'K' => 'Ķ',
        'L' => 'Ł',
        'M' => 'Ṁ',
        'N' => 'Ñ',
        'O' => 'Ó',
        'P' => 'Þ',
        'Q' => 'Ƣ',
        'R' => 'Ŕ',
        'S' => 'Š',
        'T' => 'Ţ',
        'U' => 'Ú',
        'V' => 'Ṽ',
        'W' => 'Ŵ',
        'X' => 'Ẋ',
        'Y' => 'Ý',
        'Z' => 'Ž',
        other => other,
    }
}

/// Pseudolocalise `text`: accent it, expand it to [`EXPANSION_PERCENT`], and
/// fence it in [`OPEN`] / [`CLOSE`].
///
/// See the [module documentation](self) for why each of the three matters. The
/// empty string is returned as a bare pair of fences — an empty label is still a
/// label, and its fences still say whether it was clipped.
pub(crate) fn pseudolocalise(text: &str) -> String {
    let original: usize = text.chars().count();
    // Counted in `char`s, not bytes: the accented output is multi-byte, and a
    // byte-length target would over-pad by a factor of two on Latin text and do
    // something meaningless on CJK.
    let target = original
        .saturating_mul(EXPANSION_PERCENT)
        .saturating_div(100);
    let padding = target.saturating_sub(original);

    let mut out = String::new();
    out.push(OPEN);
    out.extend(text.chars().map(accent));
    // The filler is broken into short runs separated by spaces, and that is not
    // cosmetic: a solid run of `padding` dots is a single unbreakable token, so
    // the line cannot wrap inside it and the text overflows its box no matter how
    // much room it is given. The result is a pseudolocalised label that fails
    // every layout check for a reason that is entirely the fault of the
    // pseudolocaliser — a false positive that would train everyone to ignore the
    // real ones. Found by the matrix the first time it ran against
    // `spawn_label`, which is a fair demonstration of why the matrix exists.
    //
    // A space is also pushed before the filler so it cannot be read as part of
    // the final real word, which would lose that word's wrap point too.
    if padding > 0 {
        for index in 0..padding {
            if index % PAD_RUN == 0 {
                out.push(' ');
            }
            out.push(PAD);
        }
    }
    out.push(CLOSE);
    out
}

#[cfg(test)]
mod tests {
    use super::{CLOSE, EXPANSION_PERCENT, OPEN, PAD, pseudolocalise};
    use pretty_assertions::{assert_eq, assert_ne};

    /// The three things the transform promises, on one ordinary string.
    #[test]
    fn pseudolocalisation_accents_expands_and_fences() {
        let out = pseudolocalise("Save");
        assert!(out.starts_with(OPEN), "{out} must be fenced at the start");
        assert!(out.ends_with(CLOSE), "{out} must be fenced at the end");
        assert!(
            out.contains("Šáṽé"),
            "{out} must carry the accented original"
        );
        assert!(out.contains(PAD), "{out} must be padded out");
    }

    /// The expansion actually reaches the length it claims: a transform that
    /// fenced and accented but did not lengthen would pass every eyeball test and
    /// catch none of the overflow bugs it exists for.
    #[test]
    fn pseudolocalisation_expands_by_the_stated_proportion() {
        for original in ["Save", "A somewhat longer label than that one", "x"] {
            let out = pseudolocalise(original);
            let before = original.chars().count();
            let after = out.chars().count();
            let wanted = before * EXPANSION_PERCENT / 100;
            assert!(
                after > before,
                "{original:?} -> {out:?}: pseudolocalisation must lengthen the string"
            );
            // The fences and the separating space are overhead on top of the
            // expanded text, so the result is at least the target.
            assert!(
                after >= wanted,
                "{original:?} -> {out:?}: {after} chars is under the {wanted}-char target"
            );
        }
    }

    /// Non-ASCII passes through unaccented. A CJK or Arabic sample has nothing
    /// to accent, and mangling it would test the mangling rather than the UI.
    #[test]
    fn pseudolocalisation_leaves_non_ascii_alone() {
        let out = pseudolocalise("日本語");
        assert!(
            out.contains("日本語"),
            "{out} must carry the original CJK unchanged"
        );
    }

    /// Every accented letter must actually differ from its input, or the
    /// non-ASCII claim above is silently untrue for that letter — a table this
    /// long is exactly where a copy-paste slip hides.
    #[test]
    fn every_ascii_letter_is_accented() {
        for letter in ('a'..='z').chain('A'..='Z') {
            let accented = super::accent(letter);
            assert_ne!(
                accented, letter,
                "{letter} must map to a look-alike outside ASCII"
            );
            assert!(
                !accented.is_ascii(),
                "{letter} maps to {accented}, which is still ASCII — it would not catch a \
                 font that stops at ASCII"
            );
        }
    }

    /// The accent map must be injective. Two letters mapping to the same
    /// look-alike would make the pseudo text ambiguous to the human reading the
    /// gallery, which is the one thing the accents exist to preserve.
    #[test]
    fn the_accent_map_is_injective() {
        let mut seen: Vec<char> = ('a'..='z').chain('A'..='Z').map(super::accent).collect();
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            total,
            "two ASCII letters map to the same accented character"
        );
    }

    /// An empty string is still fenced, so an empty label is still visibly a
    /// label rather than nothing at all.
    #[test]
    fn the_empty_string_is_still_fenced() {
        assert_eq!(pseudolocalise(""), format!("{OPEN}{CLOSE}"));
    }
}
