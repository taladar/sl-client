//! What the collator reads: Mono's own collation table and what en-US adds
//! to it, both vendored in `mono/` (provenance in `mono/README.md`).
//!
//! `collation.core.bin` is the resource `MSCompatUnicodeTable` loads out of
//! `mscorlib.dll` — a version byte, then five sections (ignorable flags,
//! category, levels 1 to 3), each a little-endian `u32` length and that many
//! bytes, each indexed through a [`Indexer`] of its own
//! (`MSCompatUnicodeTableUtil`). `culture-en-us.txt` is what
//! `dump_collation.cs` read off the running Mono: en-US's lower-case map,
//! which code units .NET counts unassigned, and the collator's contractions.

use std::sync::LazyLock;

/// The vendored core table.
const CORE: &[u8] = include_bytes!("../../../mono/collation.core.bin");

/// The vendored en-US data.
const CULTURE: &str = include_str!("../../../mono/culture-en-us.txt");

/// The version byte `MSCompatUnicodeTableUtil.ResourceVersion` expects.
const RESOURCE_VERSION: u8 = 3;

/// `CodePointIndexer`: the code-unit ranges a section stores, back to back,
/// and the index a unit outside them reads.
#[derive(Debug)]
pub(super) struct Indexer {
    /// `(start, end)` of each stored range, end exclusive, ascending.
    ranges: &'static [(u32, u32)],
    /// The index of a unit no range holds; `None` for `-1`, which reads
    /// nothing.
    default: Option<usize>,
}

impl Indexer {
    /// `CodePointIndexer.ToIndex`.
    fn index(&self, unit: u32) -> Option<usize> {
        let mut base = 0_u32;
        for (start, end) in self.ranges {
            if unit < *start {
                return self.default;
            }
            if unit < *end {
                return usize::try_from(unit.saturating_sub(*start).saturating_add(base)).ok();
            }
            base = base.saturating_add(end.saturating_sub(*start));
        }
        self.default
    }

    /// How many entries the section holds.
    #[cfg(test)]
    pub(super) fn total(&self) -> usize {
        self.ranges
            .iter()
            .map(|(start, end)| usize::try_from(end.saturating_sub(*start)).unwrap_or(0))
            .sum()
    }
}

/// `MSCompatUnicodeTableUtil.Ignorable`.
pub(super) const IGNORABLE: Indexer = Indexer {
    ranges: &[(0, 0x3400), (0xA000, 0xA500), (0xF900, 0x1_0000)],
    default: None,
};

/// `MSCompatUnicodeTableUtil.Category`, which `Level1` shares.
pub(super) const CATEGORY: Indexer = Indexer {
    ranges: &[
        (0, 0x1200),
        (0x1E00, 0x2800),
        (0x3000, 0x3400),
        (0x4E00, 0xA000),
        (0xAC00, 0xD7B0),
        (0xF900, 0x1_0000),
    ],
    default: Some(0),
};

/// `MSCompatUnicodeTableUtil.Level2`.
pub(super) const LEVEL2: Indexer = Indexer {
    ranges: &[
        (0, 0xF00),
        (0x1E00, 0x2800),
        (0x3000, 0x3400),
        (0xFB00, 0x1_0000),
    ],
    default: Some(0),
};

/// `MSCompatUnicodeTableUtil.Level3`.
pub(super) const LEVEL3: Indexer = Indexer {
    ranges: &[
        (0, 0x1200),
        (0x1E00, 0x2800),
        (0x3000, 0x3400),
        (0xFB00, 0x1_0000),
    ],
    default: Some(0),
};

/// Everything the collator looks up.
#[derive(Debug)]
pub(super) struct Tables {
    /// The ignorable flags: 1 ignored by every culture-sensitive
    /// comparison, 2 a symbol, 4 a non-spacing mark.
    pub(super) ignorable: &'static [u8],
    /// The category: 0 nothing, 1 a diacritic, 6 a special (variable)
    /// weight, the script's group otherwise.
    pub(super) category: &'static [u8],
    /// The primary weight.
    pub(super) level1: &'static [u8],
    /// The secondary (diacritic) weight.
    pub(super) level2: &'static [u8],
    /// The tertiary (case and width) weight.
    pub(super) level3: &'static [u8],
    /// en-US `TextInfo.ToLower`, every code unit.
    pub(super) lower: Vec<u16>,
    /// `char.GetUnicodeCategory(unit) == OtherNotAssigned`, one bit per code
    /// unit.
    pub(super) unassigned: Vec<u64>,
    /// The collator's contractions: a code unit the collator reads as the
    /// units of its replacement (`æ` as `ae`), in code-unit order.
    pub(super) contractions: Vec<(u16, Vec<u16>)>,
}

impl Tables {
    /// The section of `CORE` starting at `at`, and where the next starts;
    /// empty past the end.
    #[expect(
        clippy::little_endian_bytes,
        reason = "Mono's resource stores each section's length little-endian \
                  (MSCompatUnicodeTable.UInt32FromBytePtr)"
    )]
    fn section(at: usize) -> (&'static [u8], usize) {
        let length = CORE
            .get(at..at.saturating_add(4))
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map_or(0, u32::from_le_bytes);
        let start = at.saturating_add(4);
        let end = start.saturating_add(usize::try_from(length).unwrap_or(0));
        (CORE.get(start..end).unwrap_or_default(), end)
    }

    /// The tables, out of the vendored files. The vendored files are fixed;
    /// the tests hold them to the layout, so nothing here can fail at run
    /// time — a section that did not parse would read as empty.
    fn load() -> Self {
        let versioned = CORE.first() == Some(&RESOURCE_VERSION);
        let (ignorable, next) = Self::section(if versioned { 1 } else { CORE.len() });
        let (category, next) = Self::section(next);
        let (level1, next) = Self::section(next);
        let (level2, next) = Self::section(next);
        let (level3, _) = Self::section(next);
        let mut lower: Vec<u16> = (0..=u16::MAX).collect();
        let mut unassigned = vec![0_u64; 1024];
        let mut contractions = Vec::new();
        for line in CULTURE.lines().filter(|line| !line.starts_with('#')) {
            let mut words = line.split_whitespace();
            let kind = words.next();
            let numbers: Vec<i64> = words
                .enumerate()
                .filter_map(|(at, word)| {
                    // Every number is hexadecimal but a `lower` line's step and
                    // delta, which are decimal.
                    if kind == Some("lower") && at >= 2 {
                        word.parse().ok()
                    } else {
                        i64::from_str_radix(word, 16).ok()
                    }
                })
                .collect();
            let unit = |at: usize| numbers.get(at).and_then(|value| u16::try_from(*value).ok());
            match (kind, unit(0), unit(1)) {
                (Some("lower"), Some(first), Some(last)) => {
                    let step = numbers.get(2).and_then(|step| usize::try_from(*step).ok());
                    let delta = numbers.get(3).copied().unwrap_or(0);
                    for from in (first..=last).step_by(step.unwrap_or(1).max(1)) {
                        let to = i64::from(from).saturating_add(delta);
                        if let (Some(slot), Ok(to)) =
                            (lower.get_mut(usize::from(from)), u16::try_from(to))
                        {
                            *slot = to;
                        }
                    }
                }
                (Some("unassigned"), Some(first), Some(last)) => {
                    for code in first..=last {
                        if let Some(word) = unassigned.get_mut(usize::from(code >> 6)) {
                            *word |= 1_u64 << (code & 63);
                        }
                    }
                }
                (Some("contraction"), Some(source), _) => {
                    let replacement = (1..numbers.len()).filter_map(unit).collect();
                    contractions.push((source, replacement));
                }
                _ => {}
            }
        }
        contractions.sort_by_key(|(source, _)| *source);
        Self {
            ignorable,
            category,
            level1,
            level2,
            level3,
            lower,
            unassigned,
            contractions,
        }
    }

    /// The entry of `table` for `unit` through `indexer`, 0 for none.
    pub(super) fn lookup(table: &[u8], indexer: &Indexer, unit: u32) -> u8 {
        indexer
            .index(unit)
            .and_then(|index| table.get(index))
            .copied()
            .unwrap_or(0)
    }

    /// The ignorable flags of `unit`, or `None` where the table holds none.
    pub(super) fn ignorable_flags(&self, unit: u32) -> Option<u8> {
        IGNORABLE
            .index(unit)
            .and_then(|index| self.ignorable.get(index))
            .copied()
    }

    /// Whether .NET counts `unit` unassigned.
    pub(super) fn is_unassigned(&self, unit: u32) -> bool {
        usize::try_from(unit >> 6)
            .ok()
            .and_then(|word| self.unassigned.get(word))
            .is_some_and(|word| word & (1_u64 << (unit & 63)) != 0)
    }

    /// en-US `TextInfo.ToLower` of `unit`.
    pub(super) fn to_lower(&self, unit: u32) -> u32 {
        usize::try_from(unit)
            .ok()
            .and_then(|at| self.lower.get(at))
            .map_or(unit, |lower| u32::from(*lower))
    }
}

/// The tables, loaded once.
pub(super) static TABLES: LazyLock<Tables> = LazyLock::new(Tables::load);
