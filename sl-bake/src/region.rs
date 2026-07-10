//! The eleven avatar bake regions and their [`sl_proto`] baked-slot mapping.

use sl_proto::avatar_texture;

/// One of the avatar bake regions the client composites: the six base-body
/// (non-"universal") bakes plus the five "universal" bakes (left-arm / left-leg /
/// aux1–3) a modern mesh body samples via bake-on-mesh. Each maps to a baked
/// texture slot in an avatar `TextureEntry` (see [`avatar_texture`]), in the
/// reference viewer's `EBakedTextureIndex` order.
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
    /// The universal left-arm bake (`BAKED_LEFT_ARM`), slot
    /// [`avatar_texture::LEFT_ARM_BAKED`].
    LeftArm,
    /// The universal left-leg bake (`BAKED_LEFT_LEG`), slot
    /// [`avatar_texture::LEFT_LEG_BAKED`].
    LeftLeg,
    /// The universal aux1 bake (`BAKED_AUX1`), slot [`avatar_texture::AUX1_BAKED`].
    Aux1,
    /// The universal aux2 bake (`BAKED_AUX2`), slot [`avatar_texture::AUX2_BAKED`].
    Aux2,
    /// The universal aux3 bake (`BAKED_AUX3`), slot [`avatar_texture::AUX3_BAKED`].
    Aux3,
}

impl BakeRegion {
    /// Every avatar bake region, in `EBakedTextureIndex` order (the six base-body
    /// bakes then the five universal ones).
    pub const ALL: [Self; 11] = [
        Self::Head,
        Self::UpperBody,
        Self::LowerBody,
        Self::Eyes,
        Self::Skirt,
        Self::Hair,
        Self::LeftArm,
        Self::LeftLeg,
        Self::Aux1,
        Self::Aux2,
        Self::Aux3,
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
            Self::LeftArm => avatar_texture::LEFT_ARM_BAKED,
            Self::LeftLeg => avatar_texture::LEFT_LEG_BAKED,
            Self::Aux1 => avatar_texture::AUX1_BAKED,
            Self::Aux2 => avatar_texture::AUX2_BAKED,
            Self::Aux3 => avatar_texture::AUX3_BAKED,
        }
    }

    /// The region for an avatar-`TextureEntry` baked-slot index, or `None` for a
    /// non-baked slot. The inverse of [`BakeRegion::slot`].
    #[must_use]
    pub const fn from_slot(slot: usize) -> Option<Self> {
        Some(match slot {
            avatar_texture::HEAD_BAKED => Self::Head,
            avatar_texture::UPPER_BAKED => Self::UpperBody,
            avatar_texture::LOWER_BAKED => Self::LowerBody,
            avatar_texture::EYES_BAKED => Self::Eyes,
            avatar_texture::SKIRT_BAKED => Self::Skirt,
            avatar_texture::HAIR_BAKED => Self::Hair,
            avatar_texture::LEFT_ARM_BAKED => Self::LeftArm,
            avatar_texture::LEFT_LEG_BAKED => Self::LeftLeg,
            avatar_texture::AUX1_BAKED => Self::Aux1,
            avatar_texture::AUX2_BAKED => Self::Aux2,
            avatar_texture::AUX3_BAKED => Self::Aux3,
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
            Self::LeftArm => "leftarm",
            Self::LeftLeg => "leftleg",
            Self::Aux1 => "aux1",
            Self::Aux2 => "aux2",
            Self::Aux3 => "aux3",
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
    fn universal_bake_slots_round_trip() {
        // The universal (left-arm / left-leg / aux) bakes are now regions too.
        assert_eq!(
            BakeRegion::from_slot(sl_proto::avatar_texture::LEFT_ARM_BAKED),
            Some(BakeRegion::LeftArm)
        );
        assert_eq!(
            BakeRegion::from_slot(sl_proto::avatar_texture::AUX3_BAKED),
            Some(BakeRegion::Aux3)
        );
    }

    #[test]
    fn non_baked_slots_have_no_region() {
        // An ordinary (non-baked) slot has no bake region.
        assert_eq!(BakeRegion::from_slot(0), None);
        assert_eq!(
            BakeRegion::from_slot(sl_proto::avatar_texture::LEFT_ARM_TATTOO),
            None
        );
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
