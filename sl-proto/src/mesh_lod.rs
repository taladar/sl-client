//! The Second Life mesh level-of-detail newtype.
//!
//! Unlike a texture's progressive [`DiscardLevel`](crate::DiscardLevel) (a
//! smooth resolution scale where a *prefix* of the codestream is a coarser
//! image), a mesh asset carries **four discrete, independently stored geometry
//! blocks** — one per level of detail. Each block is a separate byte range of
//! the asset, decoded on its own; there is no progressive reuse and no
//! downsample between them.
//!
//! [`MeshLod`] names those four levels. Ordering follows resolution: a *higher*
//! level is *finer* (more detailed), so [`MeshLod::High`] is the maximum and
//! [`MeshLod::Lowest`] the minimum. "At least as fine as" is therefore `>=` —
//! the opposite sense to [`DiscardLevel`](crate::DiscardLevel), whose smaller
//! values are finer.

/// The number of discrete mesh levels of detail (`LLModel::NUM_LODS`).
pub const MESH_LOD_COUNT: usize = 4;

/// A Second Life mesh level of detail: one of the four discrete geometry blocks
/// a mesh asset carries. [`High`](Self::High) is the finest (most detailed);
/// [`Lowest`](Self::Lowest) the coarsest.
///
/// The variants are declared coarsest-first so the derived [`Ord`] matches
/// resolution: `Lowest < Low < Medium < High`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum MeshLod {
    /// The coarsest level (`lowest_lod`, block index 0).
    Lowest,
    /// The low level (`low_lod`, block index 1).
    Low,
    /// The medium level (`medium_lod`, block index 2).
    Medium,
    /// The finest, full-detail level (`high_lod`, block index 3).
    High,
}

impl MeshLod {
    /// The coarsest supported level ([`MeshLod::Lowest`]).
    pub const COARSEST: Self = Self::Lowest;

    /// The finest supported level ([`MeshLod::High`]).
    pub const FINEST: Self = Self::High;

    /// All four levels, coarsest to finest.
    pub const ALL: [Self; MESH_LOD_COUNT] = [Self::Lowest, Self::Low, Self::Medium, Self::High];

    /// The block index of this level (`0` = lowest … `3` = high), matching the
    /// viewer's `LLModel` LOD array order.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Lowest => 0,
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
        }
    }

    /// The level for a block index (`0..=3`), or `None` if out of range.
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::Lowest),
            1 => Some(Self::Low),
            2 => Some(Self::Medium),
            3 => Some(Self::High),
            _other => None,
        }
    }

    /// The mesh-header map key naming this level's geometry block
    /// (`"lowest_lod"` / `"low_lod"` / `"medium_lod"` / `"high_lod"`).
    #[must_use]
    pub const fn header_key(self) -> &'static str {
        match self {
            Self::Lowest => "lowest_lod",
            Self::Low => "low_lod",
            Self::Medium => "medium_lod",
            Self::High => "high_lod",
        }
    }

    /// The next finer (more detailed) level, saturating at [`MeshLod::High`].
    #[must_use]
    pub const fn finer(self) -> Self {
        match self {
            Self::Lowest => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium | Self::High => Self::High,
        }
    }

    /// The next coarser (less detailed) level, saturating at [`MeshLod::Lowest`].
    #[must_use]
    pub const fn coarser(self) -> Self {
        match self {
            Self::Lowest | Self::Low => Self::Lowest,
            Self::Medium => Self::Low,
            Self::High => Self::Medium,
        }
    }

    /// Whether this level is at least as fine (detailed) as `other`. Since
    /// [`MeshLod::High`] is the maximum, this is `self >= other`.
    #[must_use]
    pub const fn is_at_least_as_fine_as(self, other: Self) -> bool {
        self.index() >= other.index()
    }

    /// The finer (more detailed) of two levels.
    #[must_use]
    pub const fn finer_of(self, other: Self) -> Self {
        if self.index() >= other.index() {
            self
        } else {
            other
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MESH_LOD_COUNT, MeshLod};
    use pretty_assertions::assert_eq;

    #[test]
    fn ordering_is_lowest_to_high() {
        assert!(MeshLod::Lowest < MeshLod::Low);
        assert!(MeshLod::Low < MeshLod::Medium);
        assert!(MeshLod::Medium < MeshLod::High);
        // High is the maximum (finest).
        assert_eq!(MeshLod::FINEST, MeshLod::High);
        assert_eq!(MeshLod::COARSEST, MeshLod::Lowest);
    }

    #[test]
    fn index_and_from_index_round_trip() {
        for (expected, lod) in MeshLod::ALL.into_iter().enumerate() {
            let index = u8::try_from(expected).unwrap_or(u8::MAX);
            assert_eq!(lod.index(), index);
            assert_eq!(MeshLod::from_index(index), Some(lod));
        }
        assert_eq!(MeshLod::ALL.len(), MESH_LOD_COUNT);
        assert_eq!(MeshLod::from_index(4), None);
    }

    #[test]
    fn header_keys_match_the_viewer() {
        assert_eq!(MeshLod::Lowest.header_key(), "lowest_lod");
        assert_eq!(MeshLod::Low.header_key(), "low_lod");
        assert_eq!(MeshLod::Medium.header_key(), "medium_lod");
        assert_eq!(MeshLod::High.header_key(), "high_lod");
    }

    #[test]
    fn finer_and_coarser_saturate() {
        assert_eq!(MeshLod::Lowest.coarser(), MeshLod::Lowest);
        assert_eq!(MeshLod::High.finer(), MeshLod::High);
        assert_eq!(MeshLod::Lowest.finer(), MeshLod::Low);
        assert_eq!(MeshLod::High.coarser(), MeshLod::Medium);
        assert_eq!(MeshLod::Medium.finer(), MeshLod::High);
        assert_eq!(MeshLod::Medium.coarser(), MeshLod::Low);
    }

    #[test]
    fn fineness_comparisons() {
        assert!(MeshLod::High.is_at_least_as_fine_as(MeshLod::Lowest));
        assert!(MeshLod::Medium.is_at_least_as_fine_as(MeshLod::Medium));
        assert!(!MeshLod::Low.is_at_least_as_fine_as(MeshLod::High));
        assert_eq!(MeshLod::Low.finer_of(MeshLod::High), MeshLod::High);
        assert_eq!(MeshLod::Medium.finer_of(MeshLod::Lowest), MeshLod::Medium);
    }
}
