//! What an operation costs beyond its one instruction when the work grows
//! with the size of what it makes.
//!
//! Charges are in the scheduler's instructions, so a measured time converts at
//! [`SCRIPT_INSTRUCTIONS_PER_SECOND`]. Every number here comes from the aditi
//! transcripts of the `vm/call-cost-lists` and `vm/call-cost-strings` probes
//! (Mono, 2026-10-27; book chapter `gridspec/lsl.md`, § *What a call costs*):
//! a call's rate at four sizes against an empty loop's rate at the same size,
//! the difference of the two periods being what one call costs.

use crate::library::BuiltinId;
use crate::value::Value;
use crate::vm::engine::SCRIPT_INSTRUCTIONS_PER_SECOND;
use crate::vm::instance::BUILTIN_CALL_COST;
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

/// What a library call's cost grows with: the length of one of its
/// arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Measure {
    /// The elements of the list at this argument.
    List(usize),
    /// The characters of the string at this argument.
    Text(usize),
    /// The comma-separated elements of the string at this argument (the
    /// probe timed `llCSV2List` by the elements of the list it was made
    /// from).
    CsvElements(usize),
}

impl Measure {
    /// The size of `args` by this measure; 0 when the argument is not there
    /// or is of another type.
    fn of(self, args: &[Value]) -> u64 {
        match self {
            Self::List(at) => match args.get(at) {
                Some(Value::List(elements)) => size(elements.len()),
                _ => 0,
            },
            Self::Text(at) => match args.get(at) {
                Some(Value::String(text) | Value::Key(text)) => size(text.chars().count()),
                _ => 0,
            },
            Self::CsvElements(at) => match args.get(at) {
                Some(Value::String(text)) => size(text.matches(',').count().saturating_add(1)),
                _ => 0,
            },
        }
    }
}

/// Reading one element out of a list (`llList2Integer`, timed): flat, about
/// 4.2 µs on every list. The committed run's 7.8 µs at one element is the
/// other run's 4.2 (book: 4.2–7.9) and is left out.
const LIST_READ: SizeCost = SizeCost {
    points: &[(10, 4_186), (500, 4_347)],
};

/// `llList2List` (timed `llList2List(L, 0, -1)`). The committed run's
/// 113 µs at ten elements is below its own 183 µs at one (book: 113–639) and
/// is left out.
const LIST_COPY: SizeCost = SizeCost {
    points: &[(1, 182_938), (100, 973_828), (500, 6_540_191)],
};

/// `llListReplaceList` (timed replacing the first element). The committed
/// run's 661 µs at one element is the outlier of the book's 63–661 and is
/// left out, so a short list costs what ten elements do.
const LIST_REPLACE: SizeCost = SizeCost {
    points: &[(10, 132_073), (100, 1_676_489), (500, 9_149_324)],
};

/// `llListSort`, ascending, stride 1.
const LIST_SORT: SizeCost = SizeCost {
    points: &[
        (1, 197_946),
        (10, 334_354),
        (100, 5_285_062),
        (500, 27_417_565),
    ],
};

/// `llListFindList` of a one-element pattern that is not there.
const LIST_FIND: SizeCost = SizeCost {
    points: &[(1, 49_991), (10, 83_139), (100, 440_175), (500, 2_483_298)],
};

/// The size costs of the string and list functions, each with what it grows
/// with: Second Life's times from the aditi transcripts of
/// `vm/call-cost-lists`, `vm/call-cost-strings` and `vm/call-cost-newer`
/// (Mono, 2026-10-27; book chapter `gridspec/lsl.md`, § *What a call
/// costs*), the time above an empty loop of one call with a list of 1, 10,
/// 100 and 500 integers or a string of 1, 10, 100 and 1000 characters.
///
/// A function the probes did not time is charged as the timed one that does
/// the same work: `llToLower` as `llToUpper`, `llUnescapeURL` as
/// `llEscapeURL`, `llBase64ToString` and the XOR functions as
/// `llStringToBase64`, `llParseStringKeepNulls` as `llParseString2List`,
/// every other single-element read as `llList2Integer`, `llListInsertList`
/// as `llListReplaceList`, the strided and sliced copies as `llList2List`,
/// `llListSortStrided` as `llListSort`, the other searches as
/// `llListFindList`, and `llHMAC` as `llComputeHash`. Every other function
/// costs the flat call alone.
const fn library_cost(id: BuiltinId) -> Option<(SizeCost, Measure)> {
    use BuiltinId as F;
    use Measure::{CsvElements, List, Text};
    let to_base64 = SizeCost {
        points: &[(1, 33_517), (10, 50_932), (100, 275_948), (1000, 2_357_771)],
    };
    let upper = SizeCost {
        points: &[(1, 14_117), (10, 19_378), (100, 86_017), (1000, 504_918)],
    };
    let escape = SizeCost {
        points: &[
            (1, 70_524),
            (10, 146_215),
            (100, 1_044_984),
            (1000, 7_190_857),
        ],
    };
    let parse = SizeCost {
        points: &[
            (1, 147_767),
            (10, 453_595),
            (100, 4_933_342),
            (1000, 19_161_885),
        ],
    };
    // The one-character run's 200.3 µs is a hair above the ten's 195.9 and
    // is taken as the ten's.
    let compute_hash = SizeCost {
        points: &[
            (1, 195_911),
            (10, 195_911),
            (100, 274_394),
            (1000, 1_150_634),
        ],
    };
    Some(match id {
        F::LlList2Integer
        | F::LlList2String
        | F::LlList2Float
        | F::LlList2Key
        | F::LlList2Vector
        | F::LlList2Rot
        | F::LlGetListEntryType => (LIST_READ, List(0)),
        F::LlList2List | F::LlList2ListStrided | F::LlList2ListSlice => (LIST_COPY, List(0)),
        F::LlListReplaceList | F::LlListInsertList => (LIST_REPLACE, List(0)),
        F::LlListSort | F::LlListSortStrided => (LIST_SORT, List(0)),
        F::LlListFindList | F::LlListFindListNext | F::LlListFindStrided => (LIST_FIND, List(0)),
        F::LlDeleteSubList => (
            SizeCost {
                points: &[
                    (1, 86_285),
                    (10, 210_151),
                    (100, 1_886_019),
                    (500, 7_167_584),
                ],
            },
            List(0),
        ),
        F::LlList2CSV => (
            SizeCost {
                points: &[
                    (1, 99_417),
                    (10, 313_276),
                    (100, 2_471_874),
                    (500, 12_777_573),
                ],
            },
            List(0),
        ),
        F::LlDumpList2String => (
            SizeCost {
                points: &[
                    (1, 102_274),
                    (10, 362_066),
                    (100, 2_904_112),
                    (500, 27_114_692),
                ],
            },
            List(0),
        ),
        F::LlListStatistics => (
            SizeCost {
                points: &[(1, 28_944), (10, 55_922), (100, 513_168), (500, 2_796_468)],
            },
            List(1),
        ),
        F::LlListRandomize => (
            SizeCost {
                points: &[
                    (1, 169_445),
                    (10, 365_696),
                    (100, 3_354_688),
                    (500, 18_887_635),
                ],
            },
            List(0),
        ),
        F::LlCSV2List => (
            SizeCost {
                points: &[
                    (1, 166_645),
                    (10, 405_534),
                    (100, 4_379_347),
                    (500, 18_885_685),
                ],
            },
            CsvElements(0),
        ),
        F::LlStringLength => (
            SizeCost {
                points: &[(1, 15_422), (10, 30_288), (100, 98_368), (1000, 670_717)],
            },
            Text(0),
        ),
        F::LlGetSubString => (
            SizeCost {
                points: &[(1, 36_055), (10, 48_745), (100, 120_761), (1000, 751_811)],
            },
            Text(0),
        ),
        F::LlSubStringIndex => (
            SizeCost {
                points: &[(1, 25_671), (10, 41_002), (100, 116_029), (1000, 766_834)],
            },
            Text(0),
        ),
        F::LlParseString2List | F::LlParseStringKeepNulls => (parse, Text(0)),
        F::LlToUpper | F::LlToLower => (upper, Text(0)),
        F::LlStringToBase64
        | F::LlBase64ToString
        | F::LlXorBase64
        | F::LlXorBase64Strings
        | F::LlXorBase64StringsCorrect => (to_base64, Text(0)),
        F::LlMD5String => (
            SizeCost {
                points: &[(1, 240_023), (10, 240_569), (100, 326_253), (1000, 964_317)],
            },
            Text(0),
        ),
        F::LlSHA1String => (
            SizeCost {
                points: &[(1, 268_514), (10, 307_447), (100, 346_942), (1000, 845_389)],
            },
            Text(0),
        ),
        // The one-character run's 382.5 µs is a hair above the ten's 381.7.
        F::LlSHA256String => (
            SizeCost {
                points: &[(1, 381_674), (10, 381_674), (100, 433_737), (1000, 953_666)],
            },
            Text(0),
        ),
        F::LlComputeHash => (compute_hash, Text(0)),
        F::LlHMAC => (compute_hash, Text(1)),
        F::LlEscapeURL | F::LlUnescapeURL => (escape, Text(0)),
        F::LlInsertString => (
            SizeCost {
                points: &[(1, 52_990), (10, 99_066), (100, 466_395), (1000, 3_774_445)],
            },
            Text(0),
        ),
        F::LlDeleteSubString => (
            SizeCost {
                points: &[(1, 32_434), (10, 81_219), (100, 453_622), (1000, 3_972_495)],
            },
            Text(0),
        ),
        F::LlStringTrim => (
            SizeCost {
                points: &[(1, 38_270), (10, 77_035), (100, 466_915), (1000, 3_927_881)],
            },
            Text(0),
        ),
        F::LlReplaceSubString => (
            SizeCost {
                points: &[
                    (1, 59_424),
                    (10, 122_274),
                    (100, 576_031),
                    (1000, 4_644_208),
                ],
            },
            Text(0),
        ),
        F::LlOrd => (
            SizeCost {
                points: &[(1, 15_940), (10, 32_809), (100, 109_901), (1000, 741_545)],
            },
            Text(0),
        ),
        F::LlHash => (
            SizeCost {
                points: &[(1, 15_371), (10, 32_734), (100, 119_884), (1000, 907_197)],
            },
            Text(0),
        ),
        _ => return None,
    })
}

/// What calling the library function `id` with `args` costs on top of
/// [`BUILTIN_CALL_COST`]: the time its arguments' size costs on the grid
/// less the flat call's (see `library_cost`), or nothing for a function
/// whose work does not grow.
#[must_use]
pub(crate) fn call(id: BuiltinId, args: &[Value]) -> u32 {
    library_cost(id).map_or(0, |(cost, measure)| {
        cost.instructions(measure.of(args))
            .saturating_sub(BUILTIN_CALL_COST)
    })
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
    fn no_library_call_gets_cheaper_as_its_argument_grows() {
        for id in crate::library::BuiltinId::ALL {
            let Some((cost, _)) = super::library_cost(id) else {
                continue;
            };
            let charges: Vec<u32> = (0..2000).map(|size| cost.instructions(size)).collect();
            assert!(
                charges
                    .windows(2)
                    .all(|pair| matches!(pair, [a, b] if a <= b)),
                "{id:?}"
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
