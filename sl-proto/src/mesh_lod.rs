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

/// The reference viewer's default `RenderVolumeLODFactor`
/// (`LLVOVolume::sLODFactor`): the object-geometry detail multiplier a larger
/// value of which keeps finer geometry to a greater distance. The viewer exposes
/// no LOD-factor setting, so it runs at this default.
pub const DEFAULT_LOD_FACTOR: f32 = 1.0;

/// The finest-detail angular-size threshold (`LLVolumeLODGroup::BASE_THRESHOLD`).
/// The four reference thresholds are `{1, 2, 8, 100} * BASE_THRESHOLD`, but
/// `getDetailFromTan` only ever compares the first three (it returns the finest
/// level for anything above the third), so only `1×`, `2×`, and `8×` are used
/// here.
const BASE_THRESHOLD: f32 = 0.03;

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

    /// Selects the level of detail an object of world bounding `radius` (metres)
    /// at camera `distance` (metres) should render at, porting the reference
    /// viewer's `LLVOVolume::calcLOD` / `computeLODDetail` /
    /// `LLVolumeLODGroup::getDetailFromTan`.
    ///
    /// `lod_factor` is the viewer's `RenderVolumeLODFactor` (default
    /// [`DEFAULT_LOD_FACTOR`]): a larger value selects finer geometry at a given
    /// apparent size (keeping detail to a greater distance). The reference's
    /// per-frame FOV-zoom adjustment (`DEFAULT_FIELD_OF_VIEW / getDefaultFOV`) is
    /// *not* applied — this matches the reference with `IgnoreFOVZoomForLODs`, so
    /// the caller passes the raw LOD factor and the near-distance ramp uses it
    /// directly (`sLODFactor`), as the reference does.
    ///
    /// `radius` is the object's full scale-vector length (`getScale().length()`,
    /// the box diagonal), **not** the half-diagonal bounding-sphere radius used
    /// for pixel area: the reference viewer's LOD thresholds were tuned against
    /// that quantity (its own comment notes the rigged path is deliberately "2x
    /// off"), so passing the diagonal reproduces the reference LOD selection.
    ///
    /// A degenerate input (a non-finite or non-positive radius, distance, or LOD
    /// factor — an object at or behind the camera, or of zero size) yields the
    /// finest level, matching the reference's "boost when really close" bias.
    #[must_use]
    pub fn for_distance(radius: f32, distance: f32, lod_factor: f32) -> Self {
        if !radius.is_finite()
            || !distance.is_finite()
            || !lod_factor.is_finite()
            || radius <= 0.0
            || distance <= 0.0
            || lod_factor <= 0.0
        {
            return Self::FINEST;
        }
        // `sDistanceFactor` is 1.0, so `distance` is unscaled here.
        let mut adjusted = distance;
        // Boost detail when very close: a quadratic ramp inside `sLODFactor * 2`
        // metres (`LLVOVolume::calcLOD`).
        let ramp_dist = lod_factor * 2.0;
        if adjusted < ramp_dist {
            adjusted *= 1.0 / ramp_dist;
            adjusted *= adjusted;
            adjusted *= ramp_dist;
        }
        // The fixed geometric constant the reference folds into the distance.
        adjusted *= core::f32::consts::PI / 3.0;
        // The tangent of (half) the angle the object subtends, scaled by the LOD
        // factor — `computeLODDetail`'s `tan_angle`.
        let tan_angle = (lod_factor * radius) / adjusted;
        Self::from_detail_tan(tan_angle)
    }

    /// Maps an object's LOD `tan_angle` (from [`for_distance`](Self::for_distance))
    /// to a level, porting `LLVolumeLODGroup::getDetailFromTan`: the first of the
    /// three ascending thresholds (`1×`, `2×`, `8×` [`BASE_THRESHOLD`]) the angle
    /// falls at or below picks that level, and anything larger is the finest
    /// level.
    #[must_use]
    fn from_detail_tan(tan_angle: f32) -> Self {
        if tan_angle <= BASE_THRESHOLD {
            Self::Lowest
        } else if tan_angle <= 2.0 * BASE_THRESHOLD {
            Self::Low
        } else if tan_angle <= 8.0 * BASE_THRESHOLD {
            Self::Medium
        } else {
            Self::High
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_LOD_FACTOR, MESH_LOD_COUNT, MeshLod};
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

    #[test]
    fn for_distance_degenerate_inputs_are_finest() {
        // At or behind the camera, zero size, or a non-positive / non-finite LOD
        // factor: render at the finest level (the reference's close-object bias).
        assert_eq!(
            MeshLod::for_distance(1.0, 0.0, DEFAULT_LOD_FACTOR),
            MeshLod::FINEST
        );
        assert_eq!(
            MeshLod::for_distance(0.0, 10.0, DEFAULT_LOD_FACTOR),
            MeshLod::FINEST
        );
        assert_eq!(MeshLod::for_distance(1.0, 10.0, 0.0), MeshLod::FINEST);
        assert_eq!(
            MeshLod::for_distance(f32::NAN, 10.0, DEFAULT_LOD_FACTOR),
            MeshLod::FINEST
        );
        assert_eq!(
            MeshLod::for_distance(1.0, f32::INFINITY, DEFAULT_LOD_FACTOR),
            MeshLod::FINEST
        );
    }

    #[test]
    fn for_distance_is_monotone_in_distance() {
        // A fixed object gets no finer as it recedes: each successive distance
        // yields a level no finer than the last, and the range spans finest to
        // coarsest.
        let radius = 4.0;
        let mut previous = MeshLod::FINEST;
        let mut saw_finest = false;
        let mut saw_coarsest = false;
        for step in 1..=200_u8 {
            let distance = f32::from(step);
            let lod = MeshLod::for_distance(radius, distance, DEFAULT_LOD_FACTOR);
            assert!(
                lod <= previous,
                "LOD rose from {previous:?} to {lod:?} as distance grew to {distance}"
            );
            saw_finest |= lod == MeshLod::FINEST;
            saw_coarsest |= lod == MeshLod::COARSEST;
            previous = lod;
        }
        assert!(saw_finest, "never selected the finest level up close");
        assert!(saw_coarsest, "never selected the coarsest level far away");
    }

    #[test]
    fn for_distance_thresholds_match_get_detail_from_tan() {
        // With the near ramp disabled (distance >= sLODFactor*2 = 2 m) and
        // lod_factor 1, `tan_angle = radius / (distance * PI/3)`. Choose radius so
        // the angle lands just inside each threshold band and confirm the level.
        let pi_third = core::f32::consts::PI / 3.0;
        // tan_angle exactly at a boundary maps to the coarser side (`<=`).
        // Just above 0.03 -> Low; solve radius = tan * distance * PI/3.
        let distance = 10.0;
        let low = 0.031 * distance * pi_third;
        assert_eq!(
            MeshLod::for_distance(low, distance, DEFAULT_LOD_FACTOR),
            MeshLod::Low
        );
        let medium = 0.1 * distance * pi_third;
        assert_eq!(
            MeshLod::for_distance(medium, distance, DEFAULT_LOD_FACTOR),
            MeshLod::Medium
        );
        let high = 0.5 * distance * pi_third;
        assert_eq!(
            MeshLod::for_distance(high, distance, DEFAULT_LOD_FACTOR),
            MeshLod::High
        );
        let lowest = 0.01 * distance * pi_third;
        assert_eq!(
            MeshLod::for_distance(lowest, distance, DEFAULT_LOD_FACTOR),
            MeshLod::Lowest
        );
    }

    #[test]
    fn for_distance_higher_lod_factor_keeps_more_detail() {
        // A larger RenderVolumeLODFactor selects geometry at least as fine at the
        // same apparent size.
        let radius = 3.0;
        let distance = 40.0;
        let low_quality = MeshLod::for_distance(radius, distance, 0.5);
        let high_quality = MeshLod::for_distance(radius, distance, 2.0);
        assert!(high_quality.is_at_least_as_fine_as(low_quality));
    }
}
