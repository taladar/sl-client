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

    /// The finer (more detailed) of two levels.
    #[must_use]
    pub const fn finer_of(self, other: Self) -> Self {
        if self.index() >= other.index() {
            self
        } else {
            other
        }
    }

    /// Selects the level of detail a prim of world bounding `radius` (metres) at
    /// camera `distance` (metres) should tessellate at, porting the reference
    /// viewer's `LLVOVolume::calcLOD` / `LLVolumeLODGroup::getDetailFromTan`.
    ///
    /// The reference viewer picks a volume's LOD tier *before* it matters whether
    /// that volume's geometry is client-tessellated (a prim) or asset-backed (a
    /// mesh) — the same `LLVolumeLODGroup` angular-size computation drives both.
    /// That computation is already ported in [`sl_proto::MeshLod::for_distance`],
    /// so this maps its resulting tier onto the matching [`PrimLod`] by index:
    /// both enums are declared coarsest-first with identical `0..=3` indices, so
    /// the tier maps one-to-one.
    ///
    /// `lod_factor` is the viewer's `RenderVolumeLODFactor` (default
    /// [`sl_proto::DEFAULT_LOD_FACTOR`]); `radius` is the prim's full scale-vector
    /// length (`getScale().length()`, the box diagonal), the same quantity
    /// [`sl_proto::MeshLod::for_distance`] expects (**not** the half-diagonal
    /// bounding-sphere radius used for pixel area). A degenerate input (a
    /// non-finite or non-positive radius, distance, or LOD factor) yields the
    /// finest level, matching the reference's close-object bias.
    #[must_use]
    pub fn for_distance(radius: f32, distance: f32, lod_factor: f32) -> Self {
        let tier = sl_proto::MeshLod::for_distance(radius, distance, lod_factor);
        // Both enums index 0 (coarsest) ..= 3 (finest), so the tier always maps;
        // the finest level is the natural fallback for the unreachable arm.
        Self::from_index(tier.index()).unwrap_or(Self::FINEST)
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
        assert_eq!(PrimLod::Low.finer_of(PrimLod::High), PrimLod::High);
        assert_eq!(PrimLod::Medium.finer_of(PrimLod::Lowest), PrimLod::Medium);
    }

    #[test]
    fn for_distance_degenerate_inputs_are_finest() {
        // At or behind the camera, zero size, or a non-positive / non-finite LOD
        // factor: tessellate at the finest level (the reference's close-object
        // bias, inherited from `MeshLod::for_distance`).
        assert_eq!(PrimLod::for_distance(1.0, 0.0, 1.0), PrimLod::FINEST);
        assert_eq!(PrimLod::for_distance(0.0, 10.0, 1.0), PrimLod::FINEST);
        assert_eq!(PrimLod::for_distance(1.0, 10.0, 0.0), PrimLod::FINEST);
        assert_eq!(PrimLod::for_distance(f32::NAN, 10.0, 1.0), PrimLod::FINEST);
        assert_eq!(
            PrimLod::for_distance(1.0, f32::INFINITY, 1.0),
            PrimLod::FINEST
        );
    }

    #[test]
    fn for_distance_is_monotone_in_distance() {
        // A fixed prim gets no finer as it recedes, and the range spans finest to
        // coarsest over the sampled distances.
        let radius = 4.0;
        let mut previous = PrimLod::FINEST;
        let mut saw_finest = false;
        let mut saw_coarsest = false;
        for step in 1..=200_u8 {
            let distance = f32::from(step);
            let lod = PrimLod::for_distance(radius, distance, 1.0);
            assert!(
                lod <= previous,
                "LOD rose from {previous:?} to {lod:?} as distance grew to {distance}"
            );
            saw_finest |= lod == PrimLod::FINEST;
            saw_coarsest |= lod == PrimLod::COARSEST;
            previous = lod;
        }
        assert!(saw_finest, "never selected the finest level up close");
        assert!(saw_coarsest, "never selected the coarsest level far away");
    }

    #[test]
    fn for_distance_matches_the_mesh_lod_tier() {
        // The prim tier is the mesh tier by index (same `LLVolumeLODGroup`
        // computation): confirm parity across a spread of sizes and distances.
        for &radius in &[0.5_f32, 2.0, 5.0, 12.0] {
            for step in 1..=60_u8 {
                let distance = f32::from(step);
                let tier = sl_proto::MeshLod::for_distance(radius, distance, 1.0);
                let prim = PrimLod::for_distance(radius, distance, 1.0);
                assert_eq!(prim.index(), tier.index());
            }
        }
    }
}
