//! The six base-body avatar bake regions and their [`sl_proto`] baked-slot
//! mapping.

use sl_proto::avatar_texture;

/// One of the six base-body avatar bake regions the client composites. These are
/// the standard (non-"universal") bakes, matching the reference viewer's
/// `EBakedTextureIndex` order (`BAKED_HEAD` … `BAKED_HAIR`); each maps to a
/// baked texture slot in an avatar `TextureEntry` (see [`avatar_texture`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `BakeRegion` reads clearly"
)]
pub enum BakeRegion {
    /// The head bake (`BAKED_HEAD`), slot [`avatar_texture::HEAD_BAKED`].
    Head,
    /// The upper-body bake (`BAKED_UPPER`), slot [`avatar_texture::UPPER_BAKED`].
    UpperBody,
    /// The lower-body bake (`BAKED_LOWER`), slot [`avatar_texture::LOWER_BAKED`].
    LowerBody,
    /// The eyes bake (`BAKED_EYES`), slot [`avatar_texture::EYES_BAKED`].
    Eyes,
    /// The skirt bake (`BAKED_SKIRT`), slot [`avatar_texture::SKIRT_BAKED`].
    Skirt,
    /// The hair bake (`BAKED_HAIR`), slot [`avatar_texture::HAIR_BAKED`].
    Hair,
}

impl BakeRegion {
    /// Every base-body bake region, in `EBakedTextureIndex` order.
    pub const ALL: [Self; 6] = [
        Self::Head,
        Self::UpperBody,
        Self::LowerBody,
        Self::Eyes,
        Self::Skirt,
        Self::Hair,
    ];

    /// The avatar-`TextureEntry` baked-slot index this region composites into
    /// (an [`avatar_texture`] `*_BAKED` constant), e.g.
    /// [`BakeRegion::Head`] → [`avatar_texture::HEAD_BAKED`].
    #[must_use]
    pub const fn slot(self) -> usize {
        match self {
            Self::Head => avatar_texture::HEAD_BAKED,
            Self::UpperBody => avatar_texture::UPPER_BAKED,
            Self::LowerBody => avatar_texture::LOWER_BAKED,
            Self::Eyes => avatar_texture::EYES_BAKED,
            Self::Skirt => avatar_texture::SKIRT_BAKED,
            Self::Hair => avatar_texture::HAIR_BAKED,
        }
    }

    /// The region for an avatar-`TextureEntry` baked-slot index, or `None` for a
    /// non-base-body slot (the "universal" left-arm / left-leg / aux bakes, or
    /// any non-baked slot). The inverse of [`BakeRegion::slot`].
    #[must_use]
    pub const fn from_slot(slot: usize) -> Option<Self> {
        Some(match slot {
            avatar_texture::HEAD_BAKED => Self::Head,
            avatar_texture::UPPER_BAKED => Self::UpperBody,
            avatar_texture::LOWER_BAKED => Self::LowerBody,
            avatar_texture::EYES_BAKED => Self::Eyes,
            avatar_texture::SKIRT_BAKED => Self::Skirt,
            avatar_texture::HAIR_BAKED => Self::Hair,
            _ => return None,
        })
    }

    /// A short human-readable region name (`"head"`, `"upper"`, …), matching the
    /// [`avatar_texture::BAKED`] table's labels.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::UpperBody => "upper",
            Self::LowerBody => "lower",
            Self::Eyes => "eyes",
            Self::Skirt => "skirt",
            Self::Hair => "hair",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BakeRegion;
    use pretty_assertions::assert_eq;

    #[test]
    fn slot_round_trips_through_from_slot() {
        for region in BakeRegion::ALL {
            assert_eq!(BakeRegion::from_slot(region.slot()), Some(region));
        }
    }

    #[test]
    fn non_base_body_slots_have_no_region() {
        // A "universal" bake slot and an ordinary (non-baked) slot.
        assert_eq!(
            BakeRegion::from_slot(sl_proto::avatar_texture::LEFT_ARM_BAKED),
            None
        );
        assert_eq!(BakeRegion::from_slot(0), None);
    }

    #[test]
    fn names_are_distinct_and_non_empty() {
        let mut names: Vec<&str> = BakeRegion::ALL.iter().map(|r| r.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "region names must be distinct");
        assert!(names.iter().all(|n| !n.is_empty()));
    }
}
