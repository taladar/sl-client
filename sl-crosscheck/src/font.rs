//! A 5×7 bitmap font, built in, for the labels on a contact sheet.
//!
//! A sheet whose cells are not named is a pile of pictures: "which viewer is
//! this, which scene, which frame" is the first question its reader asks, and an
//! answer that lives in a sidecar file is an answer nobody has when they paste
//! the image into an issue.
//!
//! Built in rather than loaded, and a bitmap rather than a real typeface, for
//! one reason: the sheet must render the same on a machine with no fonts
//! installed, no asset tree beside the binary and no network. A label that
//! silently disappears because a font was not found would be worse than no
//! label at all, because a reader would not know it was missing.
//!
//! The cost is the character set: capitals, digits and the punctuation a label
//! actually uses. [`draw`] upper-cases as it goes, and substitutes `?` for
//! anything else — which is visible, rather than a hole.

/// The width of a glyph, in pixels, before scaling.
pub const GLYPH_WIDTH: u32 = 5;

/// The height of a glyph, in pixels, before scaling.
pub const GLYPH_HEIGHT: u32 = 7;

/// The gap between two glyphs, in pixels, before scaling.
pub const GLYPH_GAP: u32 = 1;

/// The glyph drawn for a character the font does not have.
const REPLACEMENT: [&str; 7] = [
    ".###.", "#...#", "....#", "..##.", "..#..", ".....", "..#..",
];

/// Every glyph, as the rows it lights: `#` is ink and anything else is not.
///
/// Written as pictures rather than as bit patterns so a wrong pixel is visible
/// in a review diff instead of being a hex digit nobody checks.
const GLYPHS: &[(char, [&str; 7])] = &[
    (
        ' ',
        [
            ".....", ".....", ".....", ".....", ".....", ".....", ".....",
        ],
    ),
    (
        'A',
        [
            ".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'B',
        [
            "####.", "#...#", "#...#", "####.", "#...#", "#...#", "####.",
        ],
    ),
    (
        'C',
        [
            ".###.", "#...#", "#....", "#....", "#....", "#...#", ".###.",
        ],
    ),
    (
        'D',
        [
            "####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####.",
        ],
    ),
    (
        'E',
        [
            "#####", "#....", "#....", "####.", "#....", "#....", "#####",
        ],
    ),
    (
        'F',
        [
            "#####", "#....", "#....", "####.", "#....", "#....", "#....",
        ],
    ),
    (
        'G',
        [
            ".###.", "#...#", "#....", "#..##", "#...#", "#...#", ".###.",
        ],
    ),
    (
        'H',
        [
            "#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'I',
        [
            ".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###.",
        ],
    ),
    (
        'J',
        [
            "..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##..",
        ],
    ),
    (
        'K',
        [
            "#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#",
        ],
    ),
    (
        'L',
        [
            "#....", "#....", "#....", "#....", "#....", "#....", "#####",
        ],
    ),
    (
        'M',
        [
            "#...#", "##.##", "#.#.#", "#...#", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'N',
        [
            "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#",
        ],
    ),
    (
        'O',
        [
            ".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
        ],
    ),
    (
        'P',
        [
            "####.", "#...#", "#...#", "####.", "#....", "#....", "#....",
        ],
    ),
    (
        'Q',
        [
            ".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#",
        ],
    ),
    (
        'R',
        [
            "####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#",
        ],
    ),
    (
        'S',
        [
            ".####", "#....", "#....", ".###.", "....#", "....#", "####.",
        ],
    ),
    (
        'T',
        [
            "#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#..",
        ],
    ),
    (
        'U',
        [
            "#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
        ],
    ),
    (
        'V',
        [
            "#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#..",
        ],
    ),
    (
        'W',
        [
            "#...#", "#...#", "#...#", "#...#", "#.#.#", "##.##", "#...#",
        ],
    ),
    (
        'X',
        [
            "#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#",
        ],
    ),
    (
        'Y',
        [
            "#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#..",
        ],
    ),
    (
        'Z',
        [
            "#####", "....#", "...#.", "..#..", ".#...", "#....", "#####",
        ],
    ),
    (
        '0',
        [
            ".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###.",
        ],
    ),
    (
        '1',
        [
            "..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###.",
        ],
    ),
    (
        '2',
        [
            ".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####",
        ],
    ),
    (
        '3',
        [
            "#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###.",
        ],
    ),
    (
        '4',
        [
            "...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#.",
        ],
    ),
    (
        '5',
        [
            "#####", "#....", "####.", "....#", "....#", "#...#", ".###.",
        ],
    ),
    (
        '6',
        [
            "..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###.",
        ],
    ),
    (
        '7',
        [
            "#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#...",
        ],
    ),
    (
        '8',
        [
            ".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###.",
        ],
    ),
    (
        '9',
        [
            ".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##..",
        ],
    ),
    (
        '.',
        [
            ".....", ".....", ".....", ".....", ".....", ".##..", ".##..",
        ],
    ),
    (
        ',',
        [
            ".....", ".....", ".....", ".....", ".##..", ".##..", ".#...",
        ],
    ),
    (
        ':',
        [
            ".....", ".##..", ".##..", ".....", ".##..", ".##..", ".....",
        ],
    ),
    (
        ';',
        [
            ".....", ".##..", ".##..", ".....", ".##..", ".#...", "#....",
        ],
    ),
    (
        '-',
        [
            ".....", ".....", ".....", "#####", ".....", ".....", ".....",
        ],
    ),
    (
        '+',
        [
            ".....", "..#..", "..#..", "#####", "..#..", "..#..", ".....",
        ],
    ),
    (
        '=',
        [
            ".....", ".....", "#####", ".....", "#####", ".....", ".....",
        ],
    ),
    (
        '_',
        [
            ".....", ".....", ".....", ".....", ".....", ".....", "#####",
        ],
    ),
    (
        '/',
        [
            "....#", "....#", "...#.", "..#..", ".#...", "#....", "#....",
        ],
    ),
    (
        '<',
        [
            "...#.", "..#..", ".#...", "#....", ".#...", "..#..", "...#.",
        ],
    ),
    (
        '>',
        [
            ".#...", "..#..", "...#.", "....#", "...#.", "..#..", ".#...",
        ],
    ),
    (
        '(',
        [
            "..##.", ".#...", "#....", "#....", "#....", ".#...", "..##.",
        ],
    ),
    (
        ')',
        [
            ".##..", "...#.", "....#", "....#", "....#", "...#.", ".##..",
        ],
    ),
    (
        '[',
        [
            ".###.", ".#...", ".#...", ".#...", ".#...", ".#...", ".###.",
        ],
    ),
    (
        ']',
        [
            ".###.", "...#.", "...#.", "...#.", "...#.", "...#.", ".###.",
        ],
    ),
    (
        '!',
        [
            "..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#..",
        ],
    ),
    (
        '?',
        [
            ".###.", "#...#", "....#", "..##.", "..#..", ".....", "..#..",
        ],
    ),
    (
        '*',
        [
            ".....", "#.#.#", ".###.", "#####", ".###.", "#.#.#", ".....",
        ],
    ),
    (
        '#',
        [
            ".#.#.", ".#.#.", "#####", ".#.#.", "#####", ".#.#.", ".#.#.",
        ],
    ),
    (
        '%',
        [
            "##..#", "##..#", "...#.", "..#..", ".#...", "#..##", "#..##",
        ],
    ),
    (
        '\'',
        [
            "..#..", "..#..", ".....", ".....", ".....", ".....", ".....",
        ],
    ),
    (
        '"',
        [
            ".#.#.", ".#.#.", ".....", ".....", ".....", ".....", ".....",
        ],
    ),
];

/// The nearest character this font has to `character`.
///
/// This crate's own prose is typographic — em dashes, curly quotes, a `×` in a
/// capture size — and a label is built out of that prose. Folding them to the
/// ASCII they stand for is what a person would have typed; leaving them as
/// replacement marks would make the sheet's own title look broken.
const fn fold(character: char) -> char {
    match character {
        '—' | '–' | '‑' => '-',
        '×' => 'X',
        '“' | '”' => '"',
        '‘' | '’' => '\'',
        '…' => '.',
        '°' => '*',
        other => other,
    }
}

/// The glyph for `character`, folded and upper-cased, or the replacement.
fn glyph(character: char) -> [&'static str; 7] {
    let wanted = fold(character).to_ascii_uppercase();
    GLYPHS
        .iter()
        .find_map(|(known, rows)| (*known == wanted).then_some(*rows))
        .unwrap_or(REPLACEMENT)
}

/// How wide `text` will be when drawn at `scale`, in pixels.
#[must_use]
pub fn width(text: &str, scale: u32) -> u32 {
    let characters = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
    characters
        .saturating_mul(GLYPH_WIDTH.saturating_add(GLYPH_GAP))
        .saturating_sub(GLYPH_GAP)
        .saturating_mul(scale)
}

/// How tall a line is when drawn at `scale`, in pixels.
#[must_use]
pub const fn height(scale: u32) -> u32 {
    GLYPH_HEIGHT.saturating_mul(scale)
}

/// Draw `text` with its top-left corner at `(x, y)`, calling `ink` for every
/// pixel it lights.
///
/// The caller supplies the ink rather than the image: the sheet draws into an
/// `image` buffer and the tests draw into a grid of `bool`, and neither should
/// have to know about the other.
pub fn draw(text: &str, x: u32, y: u32, scale: u32, ink: &mut impl FnMut(u32, u32)) {
    let advance = GLYPH_WIDTH.saturating_add(GLYPH_GAP).saturating_mul(scale);
    for (position, character) in text.chars().enumerate() {
        let origin = x.saturating_add(
            u32::try_from(position)
                .unwrap_or(u32::MAX)
                .saturating_mul(advance),
        );
        for (row, pixels) in glyph(character).into_iter().enumerate() {
            for (column, pixel) in pixels.chars().enumerate() {
                if pixel != '#' {
                    continue;
                }
                let (row, column) = (
                    u32::try_from(row).unwrap_or(0),
                    u32::try_from(column).unwrap_or(0),
                );
                // One glyph pixel is a `scale`×`scale` block, so a label stays
                // legible on a sheet that is itself a downscale of two 1080p
                // frames.
                for down in 0..scale {
                    for across in 0..scale {
                        ink(
                            origin.saturating_add(
                                column.saturating_mul(scale).saturating_add(across),
                            ),
                            y.saturating_add(row.saturating_mul(scale).saturating_add(down)),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pretty_assertions::{assert_eq, assert_ne};

    use super::{GLYPH_HEIGHT, GLYPH_WIDTH, GLYPHS, draw, glyph, width};

    /// Draw into a set of lit pixels.
    fn lit(text: &str, scale: u32) -> BTreeSet<(u32, u32)> {
        let mut pixels = BTreeSet::new();
        draw(text, 0, 0, scale, &mut |x, y| {
            let _inserted = pixels.insert((x, y));
        });
        pixels
    }

    /// Every glyph is the size the layout arithmetic assumes. A row of the wrong
    /// length would shear every label after it.
    #[test]
    fn every_glyph_is_five_by_seven() {
        for (character, rows) in GLYPHS {
            assert_eq!(
                rows.len(),
                usize::try_from(GLYPH_HEIGHT).unwrap_or(usize::MAX),
                "{character:?} has the wrong number of rows"
            );
            for row in rows {
                assert_eq!(
                    row.chars().count(),
                    usize::try_from(GLYPH_WIDTH).unwrap_or(usize::MAX),
                    "a row of {character:?} is the wrong width"
                );
            }
        }
    }

    /// No character is defined twice: the second definition would be dead and
    /// the two would drift.
    #[test]
    fn no_glyph_is_defined_twice() {
        let mut seen = BTreeSet::new();
        for (character, _rows) in GLYPHS {
            assert!(
                seen.insert(*character),
                "{character:?} is defined more than once"
            );
        }
    }

    /// A label is drawn where it says it will be, and is as wide as [`width`]
    /// promises — the number the sheet reserves space by.
    #[test]
    fn a_label_fits_the_width_it_claims() {
        let pixels = lit("SL-CLIENT", 1);
        let rightmost = pixels.iter().map(|(x, _y)| *x).max().unwrap_or(0);
        assert!(
            rightmost < width("SL-CLIENT", 1),
            "the label drew past the width it reserved"
        );
        let lowest = pixels.iter().map(|(_x, y)| *y).max().unwrap_or(0);
        assert!(lowest < GLYPH_HEIGHT);
    }

    /// An unknown character is drawn as a visible replacement rather than as
    /// nothing: a label with a hole in it looks like a label that fitted.
    #[test]
    fn an_unknown_character_is_visible() {
        assert_eq!(glyph('\u{1f600}'), super::REPLACEMENT);
        assert!(!lit("\u{1f600}", 1).is_empty());
    }

    /// This crate's prose is typographic, and a label built out of it must not
    /// come out full of replacement marks: an em dash is a dash.
    #[test]
    fn typography_folds_to_the_ascii_it_stands_for() {
        assert_eq!(lit("A—B", 1), lit("A-B", 1));
        assert_eq!(lit("1920×1080", 1), lit("1920X1080", 1));
        assert_ne!(lit("A—B", 1), lit("A?B", 1));
    }

    /// Lower case is drawn, as capitals: a scenario name is lower case and must
    /// not come out as a row of replacements.
    #[test]
    fn lower_case_is_drawn_as_capitals() {
        assert_eq!(lit("catalogue", 1), lit("CATALOGUE", 1));
        assert_ne!(lit("catalogue", 1), lit("?????????", 1));
    }

    /// Scaling multiplies both dimensions, so a label on a downscaled sheet is
    /// still legible.
    #[test]
    fn scaling_grows_the_glyph_in_both_directions() {
        let single = lit("A", 1).len();
        let double = lit("A", 2).len();
        assert_eq!(double, single.saturating_mul(4));
    }
}
