//! Terraform (land editing) types for `ModifyLand`.

use sl_wire::RegionLocalParcelId;

/// The terraform operation a `ModifyLand` brush stroke applies, mirroring the
/// viewer's `E_LAND_*` action codes — the same constants LSL exposes to
/// `llModifyLand` as `LAND_LEVEL` … `LAND_REVERT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LandBrushAction {
    /// Flatten terrain toward the brush's reference height (`LAND_LEVEL`).
    #[default]
    Level,
    /// Raise terrain (`LAND_RAISE`).
    Raise,
    /// Lower terrain (`LAND_LOWER`).
    Lower,
    /// Smooth terrain (`LAND_SMOOTH`).
    Smooth,
    /// Add noise to terrain (`LAND_NOISE`).
    Noise,
    /// Revert terrain toward the region's baked heightmap (`LAND_REVERT`).
    Revert,
}

impl LandBrushAction {
    /// The wire `Action` byte (the viewer's `E_LAND_*`).
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Level => 0,
            Self::Raise => 1,
            Self::Lower => 2,
            Self::Smooth => 3,
            Self::Noise => 4,
            Self::Revert => 5,
        }
    }

    /// Classifies a `ModifyLand` `Action` byte, returning `None` for an
    /// unrecognised code.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Level),
            1 => Some(Self::Raise),
            2 => Some(Self::Lower),
            3 => Some(Self::Smooth),
            4 => Some(Self::Noise),
            5 => Some(Self::Revert),
            _ => None,
        }
    }
}

/// The terraform brush radius, matching the viewer's three land-tool sizes and
/// the `LAND_SMALL_BRUSH` / `LAND_MEDIUM_BRUSH` / `LAND_LARGE_BRUSH` LSL
/// constants. The radius in metres is sent in the `ModifyLand`
/// `ModifyBlockExtended` block; the legacy `BrushSize` index byte is deprecated
/// (the simulator uses the metre radius) but still sent for old simulators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LandBrushSize {
    /// Small brush — 1 m radius (`LAND_SMALL_BRUSH`).
    #[default]
    Small,
    /// Medium brush — 2 m radius (`LAND_MEDIUM_BRUSH`).
    Medium,
    /// Large brush — 4 m radius (`LAND_LARGE_BRUSH`).
    Large,
}

impl LandBrushSize {
    /// The brush radius in metres, sent in the `ModifyBlockExtended` block.
    #[must_use]
    pub const fn to_metres(self) -> f32 {
        match self {
            Self::Small => 1.0,
            Self::Medium => 2.0,
            Self::Large => 4.0,
        }
    }

    /// The legacy `BrushSize` index byte (`0`/`1`/`2`). Deprecated — modern
    /// simulators read the metre radius from [`to_metres`](Self::to_metres) —
    /// but still carried for compatibility with old simulators.
    #[must_use]
    pub const fn to_index(self) -> u8 {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
        }
    }
}

/// The region-local ground rectangle a `ModifyLand` brush stroke covers, in
/// metres measured from the region's south-west corner. The reference viewer
/// sends a zero-area rectangle (`west == east`, `south == north`) at the cursor
/// for click-drag brushing, and the selected parcel's bounding rectangle for a
/// whole-parcel edit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerraformArea {
    /// Western edge (region-local X, metres).
    pub west: f32,
    /// Southern edge (region-local Y, metres).
    pub south: f32,
    /// Eastern edge (region-local X, metres).
    pub east: f32,
    /// Northern edge (region-local Y, metres).
    pub north: f32,
}

impl TerraformArea {
    /// A new terraform area from its four region-local metre edges.
    #[must_use]
    pub const fn new(west: f32, south: f32, east: f32, north: f32) -> Self {
        Self {
            west,
            south,
            east,
            north,
        }
    }

    /// A zero-area rectangle centred on a single region-local ground point, as
    /// the viewer sends for click-drag brushing.
    #[must_use]
    pub const fn point(x: f32, y: f32) -> Self {
        Self::new(x, y, x, y)
    }
}

/// A single terraform edit, the payload of [`Session::modify_land`]. Bundles the
/// brush action and radius with the strength, reference height, and the ground
/// rectangle (and optional parcel) it applies to.
///
/// [`Session::modify_land`]: crate::Session::modify_land
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandEdit {
    /// The terraform operation to apply.
    pub action: LandBrushAction,
    /// The brush radius.
    pub brush_size: LandBrushSize,
    /// How strongly to apply the edit — the wire `Seconds` field. The viewer
    /// sends `(1 / fps) * LandBrushForce`, i.e. how long the brush is held
    /// scaled by the configured force; larger values move terrain further per
    /// message.
    pub strength: f32,
    /// The reference height the brush levels toward / starts from (the wire
    /// `Height`, a region-local Z in metres).
    pub height: f32,
    /// The parcel being edited, or `None` for an un-targeted brush stroke (the
    /// wire `LocalID` of `-1` the viewer sends for free brushing).
    pub parcel: Option<RegionLocalParcelId>,
    /// The region-local ground rectangle affected.
    pub area: TerraformArea,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{LandBrushAction, LandBrushSize, TerraformArea};

    /// Each [`LandBrushAction`] round-trips through its `E_LAND_*` wire byte.
    #[test]
    fn land_brush_action_codes_round_trip() {
        for action in [
            LandBrushAction::Level,
            LandBrushAction::Raise,
            LandBrushAction::Lower,
            LandBrushAction::Smooth,
            LandBrushAction::Noise,
            LandBrushAction::Revert,
        ] {
            assert_eq!(LandBrushAction::from_code(action.to_code()), Some(action));
        }
        assert_eq!(LandBrushAction::Level.to_code(), 0);
        assert_eq!(LandBrushAction::Revert.to_code(), 5);
        assert_eq!(LandBrushAction::from_code(6), None);
    }

    /// [`LandBrushSize`] reports the LL metre radii and legacy index bytes.
    #[test]
    fn land_brush_size_metres_and_index() {
        assert_eq!(
            LandBrushSize::Small.to_metres().to_bits(),
            1.0_f32.to_bits()
        );
        assert_eq!(
            LandBrushSize::Medium.to_metres().to_bits(),
            2.0_f32.to_bits()
        );
        assert_eq!(
            LandBrushSize::Large.to_metres().to_bits(),
            4.0_f32.to_bits()
        );
        assert_eq!(LandBrushSize::Small.to_index(), 0);
        assert_eq!(LandBrushSize::Large.to_index(), 2);
    }

    /// [`TerraformArea::point`] makes a zero-area rectangle at the point.
    #[test]
    fn terraform_area_point_is_zero_area() {
        let area = TerraformArea::point(128.0, 64.0);
        assert_eq!(area, TerraformArea::new(128.0, 64.0, 128.0, 64.0));
        assert_eq!(area.west.to_bits(), area.east.to_bits());
        assert_eq!(area.south.to_bits(), area.north.to_bits());
    }
}
