//! Which brackets pair: the data behind **brace matching**.
//!
//! The language decides what counts as a bracket — a `{` inside a string or a
//! comment is not one — so a highlighter hands over only the brackets it lexed,
//! and the pairing itself is the same for every language: a stack, where a
//! closer pairs with the nearest opener of its own kind, and a closer with none
//! is left unmatched rather than allowed to close something it does not.

use core::ops::Range;

/// The three kinds of bracket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BracketKind {
    /// `(` `)`.
    Round,
    /// `[` `]`.
    Square,
    /// `{` `}`.
    Curly,
}

impl BracketKind {
    /// The kind and side of `character`, if it is a bracket: `true` for an
    /// opener.
    #[must_use]
    pub const fn of(character: char) -> Option<(Self, bool)> {
        match character {
            '(' => Some((Self::Round, true)),
            ')' => Some((Self::Round, false)),
            '[' => Some((Self::Square, true)),
            ']' => Some((Self::Square, false)),
            '{' => Some((Self::Curly, true)),
            '}' => Some((Self::Curly, false)),
            _ => None,
        }
    }
}

/// One bracket in the source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Bracket {
    /// The bytes it covers — one, for every bracket a language here has.
    pub range: Range<usize>,
    /// Its kind.
    pub kind: BracketKind,
    /// Whether it opens.
    pub open: bool,
}

/// One opener and the closer that matches it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BracketPair {
    /// The opener.
    pub open: Range<usize>,
    /// The closer.
    pub close: Range<usize>,
    /// Their kind.
    pub kind: BracketKind,
}

/// Every bracket of a file, paired.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BracketPairs {
    /// The matched pairs, in source order of their openers.
    pairs: Vec<BracketPair>,
    /// The brackets nothing matches: an opener never closed, a closer with no
    /// opener, or one of the wrong kind.
    unmatched: Vec<Bracket>,
}

impl BracketPairs {
    /// Pair `brackets`, given in source order.
    #[must_use]
    pub fn new(brackets: impl IntoIterator<Item = Bracket>) -> Self {
        let mut pairs: Vec<BracketPair> = Vec::new();
        let mut unmatched: Vec<Bracket> = Vec::new();
        let mut open: Vec<Bracket> = Vec::new();
        for bracket in brackets {
            if bracket.open {
                open.push(bracket);
                continue;
            }
            // The nearest opener of the same kind closes, and anything opened
            // inside it and never closed is left unmatched: an unclosed `(` in
            // a block must not break the block's own braces.
            let Some(depth) = open.iter().rposition(|opener| opener.kind == bracket.kind) else {
                unmatched.push(bracket);
                continue;
            };
            unmatched.extend(open.drain(depth.saturating_add(1)..));
            if let Some(opener) = open.pop() {
                pairs.push(BracketPair {
                    open: opener.range,
                    close: bracket.range,
                    kind: bracket.kind,
                });
            }
        }
        unmatched.extend(open);
        pairs.sort_by_key(|pair| pair.open.start);
        unmatched.sort_by_key(|bracket| bracket.range.start);
        Self { pairs, unmatched }
    }

    /// The matched pairs, in source order of their openers.
    #[must_use]
    pub fn pairs(&self) -> &[BracketPair] {
        &self.pairs
    }

    /// The brackets nothing matches, in source order.
    #[must_use]
    pub fn unmatched(&self) -> &[Bracket] {
        &self.unmatched
    }

    /// The pair one of whose brackets covers byte `index`.
    #[must_use]
    pub fn pair_at(&self, index: usize) -> Option<&BracketPair> {
        self.pairs
            .iter()
            .find(|pair| pair.open.contains(&index) || pair.close.contains(&index))
    }

    /// The pair to show for a caret at byte `caret`: the bracket just **after**
    /// the caret first, then the one just before it — the order every editor
    /// that matches braces uses, so a caret between `)` and `{` shows the `{`.
    #[must_use]
    pub fn pair_at_caret(&self, caret: usize) -> Option<&BracketPair> {
        self.pair_at(caret)
            .or_else(|| caret.checked_sub(1).and_then(|before| self.pair_at(before)))
    }

    /// The innermost pair of `kind` that encloses byte `index` — strictly
    /// inside it, between the opener's end and the closer's start.
    #[must_use]
    pub fn enclosing(&self, index: usize, kind: BracketKind) -> Option<&BracketPair> {
        self.pairs
            .iter()
            .filter(|pair| pair.kind == kind && pair.open.end <= index && index <= pair.close.start)
            .max_by_key(|pair| pair.open.start)
    }

    /// Where the innermost bracket of `kind` that is still **open** at byte
    /// `index` starts — a pair around it, or an opener before it that nothing
    /// has closed yet, which is what a closer typed at `index` would close.
    #[must_use]
    pub fn open_at(&self, index: usize, kind: BracketKind) -> Option<Range<usize>> {
        let paired = self.enclosing(index, kind).map(|pair| pair.open.clone());
        let unclosed = self
            .unmatched
            .iter()
            .filter(|bracket| bracket.open && bracket.kind == kind && bracket.range.end <= index)
            .map(|bracket| bracket.range.clone())
            .max_by_key(|range| range.start);
        paired
            .into_iter()
            .chain(unclosed)
            .max_by_key(|range| range.start)
    }
}

#[cfg(test)]
mod tests {
    use super::{Bracket, BracketKind, BracketPairs};
    use pretty_assertions::assert_eq;

    /// The brackets of `source`, every bracket character counted.
    fn pairs_of(source: &str) -> BracketPairs {
        BracketPairs::new(source.char_indices().filter_map(|(index, character)| {
            BracketKind::of(character).map(|(kind, open)| Bracket {
                range: index..index.saturating_add(1),
                kind,
                open,
            })
        }))
    }

    /// Nested pairs pair inside out; the caret prefers the bracket after it.
    #[test]
    fn nested_pairs_and_the_caret() {
        let pairs = pairs_of("f(a[1]){x}");
        let found: Vec<(usize, usize)> = pairs
            .pairs()
            .iter()
            .map(|pair| (pair.open.start, pair.close.start))
            .collect();
        assert_eq!(found, vec![(1, 6), (3, 5), (7, 9)]);
        assert!(pairs.unmatched().is_empty());
        // Between `)` and `{`: the `{` after the caret wins.
        assert_eq!(pairs.pair_at_caret(7).map(|pair| pair.close.start), Some(9));
        // Past the end of `}`: the one before it.
        assert_eq!(pairs.pair_at_caret(10).map(|pair| pair.open.start), Some(7));
        assert_eq!(pairs.pair_at_caret(0), None);
        assert_eq!(
            pairs
                .enclosing(4, BracketKind::Round)
                .map(|pair| pair.open.start),
            Some(1)
        );
    }

    /// A closer of a kind nothing opened closes nothing; an opener never
    /// closed is unmatched, and does not stop its block's braces pairing.
    #[test]
    fn mismatches_are_left_unmatched() {
        let pairs = pairs_of("{ ( ] }");
        let unmatched: Vec<usize> = pairs
            .unmatched()
            .iter()
            .map(|bracket| bracket.range.start)
            .collect();
        assert_eq!(unmatched, vec![2, 4]);
        // A closer typed after an unclosed opener would close it.
        let typing = pairs_of("a { b { } ");
        assert_eq!(
            typing
                .open_at(10, BracketKind::Curly)
                .map(|range| range.start),
            Some(2)
        );
        assert_eq!(
            typing
                .open_at(7, BracketKind::Curly)
                .map(|range| range.start),
            Some(6)
        );
        assert_eq!(
            pairs
                .pair_at(6)
                .map(|pair| (pair.open.start, pair.close.start)),
            Some((0, 6))
        );
    }
}
