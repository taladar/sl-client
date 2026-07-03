//! The Second Life prim level-of-detail newtype and its detail→step-count map.
//!
//! Unlike a mesh's four independently stored geometry blocks
//! ([`sl_proto::MeshLod`]) or a texture's progressive discard levels, a prim's
//! geometry is *tessellated on the client* at a chosen detail. The viewer
//! selects one of four detail multipliers by the prim's on-screen size
//! (`LLVolumeLODGroup`); [`PrimLod`] names those four levels.
//!
//! The detail multiplier scales every step count in the tessellation: the
//! reference viewer builds an `n`-gon profile ring with roughly
//! [`MIN_DETAIL_FACES`]` * detail` sides (Firestorm `LLProfile::genNGon` /
//! `LLPath::genNGon`, `MIN_DETAIL_FACES = 6`). Higher levels are finer, so
//! [`PrimLod::High`] is the maximum, matching [`sl_proto::MeshLod`]'s ordering.
//!
//! [`sl_proto::MeshLod`]: https://docs.rs/sl-proto

/// The number of discrete prim levels of detail (`LLVolumeLODGroup::NUM_LODS`).
pub const PRIM_LOD_COUNT: usize = 4;

/// The base side count of a full circular profile at detail `1.0`
/// (Firestorm `MIN_DETAIL_FACES`); the ring side count is
/// [`round`](f32::round)`(MIN_DETAIL_FACES * detail)`.
pub const MIN_DETAIL_FACES: f32 = 6.0;

/// A prim level of detail: one of the four client-tessellation detail levels the
/// viewer picks by on-screen size. [`High`](Self::High) is the finest (most
/// steps), [`Lowest`](Self::Lowest) the coarsest.
///
/// The variants are declared coarsest-first so the derived [`Ord`] matches
/// resolution: `Lowest < Low < Medium < High`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `PrimLod` reads clearly"
)]
pub enum PrimLod {
    /// The coarsest level (detail multiplier `1.0`, block index `0`).
    Lowest,
    /// The low level (detail multiplier `1.5`, block index `1`).
    Low,
    /// The medium level (detail multiplier `2.5`, block index `2`).
    Medium,
    /// The finest, full-detail level (detail multiplier `4.0`, block index `3`).
    High,
}

impl PrimLod {
    /// The coarsest supported level ([`PrimLod::Lowest`]).
    pub const COARSEST: Self = Self::Lowest;

    /// The finest supported level ([`PrimLod::High`]).
    pub const FINEST: Self = Self::High;

    /// All four levels, coarsest to finest.
    pub const ALL: [Self; PRIM_LOD_COUNT] = [Self::Lowest, Self::Low, Self::Medium, Self::High];

    /// The block index of this level (`0` = lowest … `3` = high), matching the
    /// reference viewer's `detail[]` array order.
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

    /// The tessellation detail multiplier for this level: the reference viewer's
    /// `F32 detail[] = {1.f, 1.5f, 2.5f, 4.f}` (Firestorm
    /// `LLVolumeParams`/`LLVolumeLODGroup`). Every profile / path step count is
    /// scaled by it.
    #[must_use]
    pub const fn detail(self) -> f32 {
        match self {
            Self::Lowest => 1.0,
            Self::Low => 1.5,
            Self::Medium => 2.5,
            Self::High => 4.0,
        }
    }

    /// The number of sides a full circular profile ring is built with at this
    /// level: [`round`](f32::round)`(`[`MIN_DETAIL_FACES`]` * detail)`, matching
    /// the reference viewer's `circle_detail = MIN_DETAIL_FACES * detail`. A
    /// straight-sided profile (square / triangle) ignores this; it applies to
    /// circle, half-circle, and hollow curves.
    #[must_use]
    pub fn circle_sides(self) -> u32 {
        round_to_u32(MIN_DETAIL_FACES * self.detail())
    }

    /// The next finer (more detailed) level, saturating at [`PrimLod::High`].
    #[must_use]
    pub const fn finer(self) -> Self {
        match self {
            Self::Lowest => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium | Self::High => Self::High,
        }
    }

    /// The next coarser (less detailed) level, saturating at
    /// [`PrimLod::Lowest`].
    #[must_use]
    pub const fn coarser(self) -> Self {
        match self {
            Self::Lowest | Self::Low => Self::Lowest,
            Self::Medium => Self::Low,
            Self::High => Self::Medium,
        }
    }

    /// Whether this level is at least as fine (detailed) as `other`. Since
    /// [`PrimLod::High`] is the maximum, this is `self >= other`.
    #[must_use]
    pub const fn is_at_least_as_fine_as(self, other: Self) -> bool {
        self.index() >= other.index()
    }
}

/// Rounds a small, non-negative side count (`MIN_DETAIL_FACES * detail`, at most
/// `24`) to the nearest `u32`. There is no `f32 → u32` conversion without a
/// cast, and the input is bounded well within `u32`, so the cast lints are
/// expected here.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "input is a non-negative side count <= 24; nearest-integer rounding fits a u32 exactly"
)]
const fn round_to_u32(value: f32) -> u32 {
    value.round().max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::{MIN_DETAIL_FACES, PRIM_LOD_COUNT, PrimLod};
    use pretty_assertions::assert_eq;

    #[test]
    fn ordering_is_lowest_to_high() {
        assert!(PrimLod::Lowest < PrimLod::Low);
        assert!(PrimLod::Low < PrimLod::Medium);
        assert!(PrimLod::Medium < PrimLod::High);
        assert_eq!(PrimLod::FINEST, PrimLod::High);
        assert_eq!(PrimLod::COARSEST, PrimLod::Lowest);
    }

    #[test]
    fn index_and_from_index_round_trip() {
        for (expected, lod) in PrimLod::ALL.into_iter().enumerate() {
            let index = u8::try_from(expected).unwrap_or(u8::MAX);
            assert_eq!(lod.index(), index);
            assert_eq!(PrimLod::from_index(index), Some(lod));
        }
        assert_eq!(PrimLod::ALL.len(), PRIM_LOD_COUNT);
        assert_eq!(PrimLod::from_index(4), None);
    }

    #[test]
    fn detail_matches_the_viewer_multipliers() {
        // Bit-exact: these are the literal viewer multipliers, not computed.
        assert_eq!(PrimLod::Lowest.detail().to_bits(), 1.0_f32.to_bits());
        assert_eq!(PrimLod::Low.detail().to_bits(), 1.5_f32.to_bits());
        assert_eq!(PrimLod::Medium.detail().to_bits(), 2.5_f32.to_bits());
        assert_eq!(PrimLod::High.detail().to_bits(), 4.0_f32.to_bits());
        assert_eq!(MIN_DETAIL_FACES.to_bits(), 6.0_f32.to_bits());
    }

    #[test]
    fn circle_sides_scale_with_detail() {
        // MIN_DETAIL_FACES (6) * detail, rounded.
        assert_eq!(PrimLod::Lowest.circle_sides(), 6);
        assert_eq!(PrimLod::Low.circle_sides(), 9);
        assert_eq!(PrimLod::Medium.circle_sides(), 15);
        assert_eq!(PrimLod::High.circle_sides(), 24);
    }

    #[test]
    fn finer_and_coarser_saturate() {
        assert_eq!(PrimLod::Lowest.coarser(), PrimLod::Lowest);
        assert_eq!(PrimLod::High.finer(), PrimLod::High);
        assert_eq!(PrimLod::Lowest.finer(), PrimLod::Low);
        assert_eq!(PrimLod::High.coarser(), PrimLod::Medium);
        assert_eq!(PrimLod::Medium.finer(), PrimLod::High);
        assert_eq!(PrimLod::Medium.coarser(), PrimLod::Low);
    }

    #[test]
    fn fineness_comparisons() {
        assert!(PrimLod::High.is_at_least_as_fine_as(PrimLod::Lowest));
        assert!(PrimLod::Medium.is_at_least_as_fine_as(PrimLod::Medium));
        assert!(!PrimLod::Low.is_at_least_as_fine_as(PrimLod::High));
    }
}
