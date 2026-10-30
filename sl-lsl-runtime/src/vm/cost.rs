//! What an operation costs beyond its one instruction when the work grows
//! with the size of what it makes.
//!
//! Charges are in the scheduler's instructions, so a measured time converts at
//! [`SCRIPT_INSTRUCTIONS_PER_SECOND`]. Every number here comes from the aditi
//! transcripts of the `vm/call-cost-lists` and `vm/call-cost-strings` probes
//! (Mono, 2026-10-27; book chapter `gridspec/lsl.md`, § *What a call costs*):
//! a call's rate at four sizes against an empty loop's rate at the same size,
//! the difference of the two periods being what one call costs.

use crate::value::Value;
use crate::vm::engine::SCRIPT_INSTRUCTIONS_PER_SECOND;
use sl_lsl::ast::BinaryOp;

/// A cost that grows with a size: measured points joined by straight lines,
/// flat before the first and following the last segment's slope after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SizeCost {
    /// `(size, nanoseconds)`, sizes strictly increasing; at least two.
    points: &'static [(u64, u64)],
}

impl SizeCost {
    /// The time one operation of `size` takes, in nanoseconds.
    #[must_use]
    pub(crate) fn nanoseconds(&self, size: u64) -> u64 {
        let mut segments = self.points.windows(2);
        let segment = segments
            .find(|pair| matches!(pair, [_, (upper, _)] if size <= *upper))
            .or_else(|| self.points.windows(2).last());
        let Some(&[(x0, y0), (x1, y1)]) = segment else {
            return 0;
        };
        if size <= x0 {
            return y0;
        }
        // Straight between the two points, and on past the last one along the
        // same slope. A cost never falls as the size grows.
        let rise = y1.saturating_sub(y0);
        let run = x1.saturating_sub(x0).max(1);
        let along = size.saturating_sub(x0);
        y0.saturating_add(
            u64::try_from(
                u128::from(rise)
                    .saturating_mul(u128::from(along))
                    .checked_div(u128::from(run))
                    .unwrap_or(0),
            )
            .unwrap_or(u64::MAX),
        )
    }

    /// What one operation of `size` costs, in the scheduler's instructions.
    #[must_use]
    pub(crate) fn instructions(&self, size: u64) -> u32 {
        let charged = u128::from(self.nanoseconds(size))
            .saturating_mul(u128::from(SCRIPT_INSTRUCTIONS_PER_SECOND))
            .checked_div(1_000_000_000)
            .unwrap_or(0);
        u32::try_from(charged).unwrap_or(u32::MAX)
    }
}

/// `list + list` (and a list joined with one value), by the length of the
/// list it makes.
///
/// Measured as `r = L + [-1]` with `L` a global list of 1, 10, 100 and 500
/// integers: 185.6, 197.5, 1334.6 and 7776.0 µs a call above the empty loop.
/// A new list costs Mono as much as 75 library calls before it holds
/// anything, and then about 16 µs an element: building a list with `+=` in a
/// loop is quadratic, and as slow as on the grid. The two hours-apart aditi
/// runs differed by up to a sixth at the small sizes (book: 161–186 µs at one
/// element); these are the committed run's.
pub(crate) const LIST_JOIN: SizeCost = SizeCost {
    points: &[
        (2, 185_618),
        (11, 197_516),
        (101, 1_334_644),
        (501, 7_775_998),
    ],
};

/// `string + string`, by the characters of the string it makes.
///
/// Measured as `s = S + "x"` with `S` a global string of 1, 10, 100 and 1000
/// characters: 10.8, 11.3, 38.2 and 209.5 µs a call above the empty loop —
/// four library calls before the first character, then about 0.19 µs one.
pub(crate) const STRING_JOIN: SizeCost = SizeCost {
    points: &[(2, 10_753), (11, 11_290), (101, 38_172), (1001, 209_511)],
};

/// What the operator `op` costs on top of its own instruction, given the
/// `value` it produced: a join of lists or strings grows with the result; any
/// other operator is the one instruction.
#[must_use]
pub(crate) fn operator(op: BinaryOp, value: &Value) -> u32 {
    if !matches!(op, BinaryOp::Add) {
        return 0;
    }
    match value {
        Value::List(elements) => LIST_JOIN.instructions(size(elements.len())),
        Value::String(text) => STRING_JOIN.instructions(size(text.chars().count())),
        Value::Integer(_)
        | Value::Float(_)
        | Value::Key(_)
        | Value::Vector(_)
        | Value::Rotation(_) => 0,
    }
}

/// A length as a size.
fn size(length: usize) -> u64 {
    u64::try_from(length).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{LIST_JOIN, STRING_JOIN, SizeCost};

    #[test]
    fn a_size_cost_passes_through_its_points_and_runs_on_past_the_last() {
        let cost = SizeCost {
            points: &[(2, 100), (12, 200), (22, 400)],
        };
        assert_eq!(cost.nanoseconds(0), 100);
        assert_eq!(cost.nanoseconds(2), 100);
        assert_eq!(cost.nanoseconds(7), 150);
        assert_eq!(cost.nanoseconds(12), 200);
        assert_eq!(cost.nanoseconds(17), 300);
        assert_eq!(cost.nanoseconds(22), 400);
        assert_eq!(cost.nanoseconds(32), 600);
    }

    #[test]
    fn the_joins_never_get_cheaper_as_they_grow() {
        for cost in [LIST_JOIN, STRING_JOIN] {
            let charges: Vec<u32> = (0..2000).map(|size| cost.instructions(size)).collect();
            assert!(
                charges
                    .windows(2)
                    .all(|pair| matches!(pair, [a, b] if a <= b))
            );
        }
    }

    #[test]
    fn a_microsecond_is_two_hundred_and_sixty_instructions() {
        let cost = SizeCost {
            points: &[(0, 1_000), (1, 1_000)],
        };
        assert_eq!(cost.instructions(0), 260);
    }
}
