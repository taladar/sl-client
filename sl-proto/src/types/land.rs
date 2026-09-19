//! Terraform (land editing) types for `ModifyLand`.

use sl_wire::RegionLocalParcelId;

/// The terraform operation a `ModifyLand` brush stroke applies, mirroring the
/// viewer's `E_LAND_*` action codes — the same constants LSL exposes to
/// `llModifyLand` as `LAND_LEVEL` … `LAND_REVERT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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

    /// Classifies a `ModifyBlockExtended` metre radius back into a brush size,
    /// the inverse of [`to_metres`](Self::to_metres). Returns `None` for a
    /// radius that is not one of the three viewer sizes.
    #[must_use]
    pub const fn from_metres(metres: f32) -> Option<Self> {
        match metres.to_bits() {
            bits if bits == 1.0_f32.to_bits() => Some(Self::Small),
            bits if bits == 2.0_f32.to_bits() => Some(Self::Medium),
            bits if bits == 4.0_f32.to_bits() => Some(Self::Large),
            _ => None,
        }
    }

    /// Classifies a legacy `BrushSize` index byte back into a brush size, the
    /// inverse of [`to_index`](Self::to_index). Returns `None` for an
    /// unrecognised index.
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::Small),
            1 => Some(Self::Medium),
            2 => Some(Self::Large),
            _ => None,
        }
    }
}

/// The terraform brush radius actually carried on the wire, in metres — the
/// `ModifyLand` `ModifyBlockExtended` block's `BrushSize` float.
///
/// [`LandBrushSize`] names the three **LSL constant** radii, and is what a
/// script or a three-way size picker deals in. The wire field is a plain float,
/// and the reference viewer's bulldozer slider runs continuously from 1 m to
/// 11 m (`LandBrushSize` in its settings, `floater_tools.xml`'s
/// `slider brush size`), so a radius arriving from — or going to — a real viewer
/// is very often none of the three. This is that value: the three constants
/// convert into it, and [`size`](Self::size) classifies one back out when it
/// happens to be exact.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct LandBrushRadius(f32);

impl LandBrushRadius {
    /// The smallest radius the reference's slider offers, in metres.
    pub const MIN_METRES: f32 = 1.0;

    /// The largest radius the reference's slider offers, in metres.
    pub const MAX_METRES: f32 = 11.0;

    /// A brush radius of `metres`, clamped into
    /// [`MIN_METRES`](Self::MIN_METRES)`..=`[`MAX_METRES`](Self::MAX_METRES).
    /// A zero or negative radius would be a no-op stroke and a huge one a
    /// region-wide edit, neither of which any viewer can ask for.
    #[must_use]
    pub const fn new(metres: f32) -> Self {
        Self(metres.clamp(Self::MIN_METRES, Self::MAX_METRES))
    }

    /// The radius in metres, as sent in the `ModifyBlockExtended` block.
    #[must_use]
    pub const fn to_metres(self) -> f32 {
        self.0
    }

    /// The legacy `BrushSize` index byte (`0`/`1`/`2`), bucketed by which of the
    /// three constant radii this one is nearest.
    ///
    /// The byte is deprecated — a modern simulator reads the metre radius from
    /// [`to_metres`](Self::to_metres) — and the reference viewer's own
    /// `LLToolBrushLand::getBrushIndex` derives it with a strict `>` loop that
    /// lands *below* the constant at each exact value (its 2 m brush sends `0`,
    /// its 4 m brush `1`). Bucketing by nearest instead keeps the three LSL
    /// constants on the bytes they are named for, which is what an old
    /// simulator reading the byte actually wants, and agrees with
    /// [`LandBrushSize::to_index`].
    #[must_use]
    pub fn to_index(self) -> u8 {
        if self.0 < 1.5 {
            0
        } else if self.0 < 3.0 {
            1
        } else {
            2
        }
    }

    /// The [`LandBrushSize`] this radius is exactly one of, or `None` for any
    /// other radius the slider can produce.
    #[must_use]
    pub const fn size(self) -> Option<LandBrushSize> {
        LandBrushSize::from_metres(self.0)
    }
}

impl Default for LandBrushRadius {
    /// The default brush, matching [`LandBrushSize::default`] — a 1 m radius.
    fn default() -> Self {
        Self(LandBrushSize::Small.to_metres())
    }
}

impl From<LandBrushSize> for LandBrushRadius {
    /// The constant size's radius in metres.
    fn from(size: LandBrushSize) -> Self {
        Self(size.to_metres())
    }
}

/// The region-local ground rectangle a `ModifyLand` brush stroke covers, in
/// metres measured from the region's south-west corner. The reference viewer
/// sends a zero-area rectangle (`west == east`, `south == north`) at the cursor
/// for click-drag brushing, and the selected parcel's bounding rectangle for a
/// whole-parcel edit.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LandEdit {
    /// The terraform operation to apply.
    pub action: LandBrushAction,
    /// The brush radius. A [`LandBrushSize`] converts in
    /// (`LandBrushSize::Large.into()`); the reference's bulldozer slider sends
    /// any radius from 1 m to 11 m.
    pub brush_radius: LandBrushRadius,
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

    use super::{LandBrushAction, LandBrushRadius, LandBrushSize, TerraformArea};

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

    /// [`LandBrushSize`] round-trips through both its metre radius and its
    /// legacy index byte, and rejects values that are neither.
    #[test]
    fn land_brush_size_decodes_metres_and_index() {
        for size in [
            LandBrushSize::Small,
            LandBrushSize::Medium,
            LandBrushSize::Large,
        ] {
            assert_eq!(LandBrushSize::from_metres(size.to_metres()), Some(size));
            assert_eq!(LandBrushSize::from_index(size.to_index()), Some(size));
        }
        assert_eq!(LandBrushSize::from_metres(3.0), None);
        assert_eq!(LandBrushSize::from_index(3), None);
    }

    /// The three [`LandBrushSize`] constants convert into a [`LandBrushRadius`]
    /// keeping both their metre radius and the legacy index byte they are named
    /// for, and classify back out of it.
    #[test]
    fn land_brush_radius_carries_the_constant_sizes() {
        for size in [
            LandBrushSize::Small,
            LandBrushSize::Medium,
            LandBrushSize::Large,
        ] {
            let radius = LandBrushRadius::from(size);
            assert_eq!(radius.to_metres().to_bits(), size.to_metres().to_bits());
            assert_eq!(radius.to_index(), size.to_index());
            assert_eq!(radius.size(), Some(size));
        }
        assert_eq!(LandBrushRadius::default(), LandBrushSize::default().into());
    }

    /// A radius off the three constants — what the reference's 1 m…11 m
    /// bulldozer slider mostly sends — survives as itself, buckets to a legacy
    /// byte, and classifies as no constant size.
    #[test]
    fn land_brush_radius_keeps_an_off_constant_slider_value() {
        let radius = LandBrushRadius::new(7.5);
        assert_eq!(radius.to_metres().to_bits(), 7.5_f32.to_bits());
        assert_eq!(radius.size(), None);
        assert_eq!(radius.to_index(), 2);
        // The buckets sit between the constants, so each constant keeps its own
        // byte and a slider value takes the nearest one.
        assert_eq!(LandBrushRadius::new(1.4).to_index(), 0);
        assert_eq!(LandBrushRadius::new(2.9).to_index(), 1);
        assert_eq!(LandBrushRadius::new(3.0).to_index(), 2);
    }

    /// A radius outside the reference slider's travel is clamped to it rather
    /// than sent as a no-op (or region-wide) stroke.
    #[test]
    fn land_brush_radius_clamps_to_the_slider_travel() {
        assert_eq!(
            LandBrushRadius::new(0.0).to_metres().to_bits(),
            LandBrushRadius::MIN_METRES.to_bits()
        );
        assert_eq!(
            LandBrushRadius::new(-3.0).to_metres().to_bits(),
            LandBrushRadius::MIN_METRES.to_bits()
        );
        assert_eq!(
            LandBrushRadius::new(1000.0).to_metres().to_bits(),
            LandBrushRadius::MAX_METRES.to_bits()
        );
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
