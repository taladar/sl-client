//! The build floater's **parameter tabs** (`viewer-prim-parameter-editing`):
//! the Object-tab editors — name & description, the physical / temporary /
//! phantom flags, the prim **shape** parameters (type, cut, hollow & hollow
//! shape, twist, taper / hole size, shear, the per-type advanced cut, taper
//! profile, radius offset, revolutions, skew) — and the Features-tab editors —
//! the physical material, the flexible path, and the light source.
//!
//! # Model
//!
//! - Everything reads the **primary selection**
//!   ([`crate::edit_selection::SelectionSet`]) through
//!   [`ObjectState::edit_data`] (shape / flags / material / extra params) and
//!   the node's `ObjectProperties` (name / description). Widgets rewrite only
//!   when that snapshot **changes** ([`ShownSnapshot`]), so a just-committed
//!   edit is not clobbered back to the pre-echo value while the simulator's
//!   confirming `ObjectUpdate` is in flight.
//! - **Commits** mirror the reference viewer's message split: name /
//!   description go out as `ObjectName` / `ObjectDescription`; the flag
//!   toggles as an `ObjectFlagUpdate`; the material cycle as `ObjectMaterial`;
//!   every shape edit rebuilds the **full** quantized [`PrimShapeParams`] from
//!   the displayed fields (the reference's `getVolumeParams` — there are no
//!   incremental shape sends) and goes out as `ObjectShape`; the flexi / light
//!   editors rebuild the object's **complete** [`ObjectExtraParams`] (the wire
//!   message states the whole set — a partial send would clear the sculpt /
//!   animesh / render-material params) and go out as `ObjectExtraParams`.
//! - The **S/T flip** (the reference's single biggest shape gotcha,
//!   `llpanelobject.cpp`): for a sphere / torus / tube / ring the "Path Cut"
//!   row edits the *path* begin/end and the advanced row ("Dimple" / "Profile
//!   Cut") the *profile* begin/end; for a box / cylinder / prism it is exactly
//!   reversed ("Slice" = path). [`ShapeField`] display values likewise scale
//!   twist by ±180° on a linear path and ±360° on a circular one, show hollow
//!   as a percentage, and show the box-family taper as `1 − ratio`.
//! - Deliberate deviations from the reference, pending their own tasks: the
//!   type / hollow-shape / material combos are **cycle buttons**
//!   ([[viewer-ui-combo-widget]]), the light colour is three numeric sRGB
//!   fields ([[viewer-ui-color-picker]]), and the sculpt texture / spotlight
//!   projector texture are not editable ([[viewer-ui-texture-picker]]) — a
//!   sculpt keeps its identity (switching a prim *to* sculpt is not offered),
//!   and an existing spotlight's FOV / focus / ambiance stay editable.
//!
//! Reference (Firestorm, read-only): `llpanelobject`, `llpanelvolume`;
//! messages `ObjectName`, `ObjectDescription`, `ObjectFlagUpdate`,
//! `ObjectMaterial`, `ObjectShape`, `ObjectExtraParams`.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy_flair::style::components::ClassList;
use bevy_fluent::Localization;
use sl_client_bevy::{
    AgentKey, Command, FlexibleData, GroupKey, HoleType, LightData, Material, ObjectExtraParams,
    ObjectFlagSettings, OwnerKey, PathCurve, PermissionField, Permissions, Permissions5,
    PrimShapeFloat, PrimShapeParams, ProfileCurve, ScopedObjectId, SlCommand, Uuid, Vector, pcode,
};

use crate::avatars::AvatarState;
use crate::edit_selection::SelectionSet;
use crate::edit_tool::{
    BuildTabPages, CHECKED_GLYPH, EditToolState, LABEL_CLASS, TOOL_FONT_SIZE, UNCHECKED_GLYPH,
    VALUE_CLASS, spawn_row_label,
};
use crate::groups::GroupsModel;
use crate::i18n::{Translated, Translator};
use crate::objects::ObjectState;
use crate::ui::{UiPanelShown, column, row};
use crate::ui_font::UiFont;
use crate::ui_text_input::{TextInputKind, TextInputSpec, TextInputValue, spawn_text_input};
use crate::web_floater::set_editor_text;

// ---------------------------------------------------------------------------
// Wire constants (the reference's `llprimitive.cpp` limits, SL variants).
// ---------------------------------------------------------------------------

/// The `FLAGS_USE_PHYSICS` bit of `PrimFlags` (`object_flags.h`).
const FLAGS_USE_PHYSICS: u32 = 1 << 0;

/// The `FLAGS_PHANTOM` bit of `PrimFlags`.
const FLAGS_PHANTOM: u32 = 1 << 4;

/// The `FLAGS_TEMPORARY_ON_REZ` bit of `PrimFlags`.
const FLAGS_TEMPORARY_ON_REZ: u32 = 1 << 29;

/// The `FLAGS_CAST_SHADOWS` legacy bit of `PrimFlags`, round-tripped so an
/// `ObjectFlagUpdate` does not silently clear it on objects that carry it.
const FLAGS_CAST_SHADOWS: u32 = 1 << 1;

/// The agent-relative `FLAGS_OBJECT_MODIFY` bit: this agent may modify.
const FLAGS_OBJECT_MODIFY: u32 = 1 << 2;

/// The agent-relative `FLAGS_OBJECT_COPY` bit.
const FLAGS_OBJECT_COPY: u32 = 1 << 3;

/// The agent-relative `FLAGS_OBJECT_MOVE` bit.
const FLAGS_OBJECT_MOVE: u32 = 1 << 8;

/// The agent-relative `FLAGS_OBJECT_TRANSFER` bit.
const FLAGS_OBJECT_TRANSFER: u32 = 1 << 17;

/// The group-share mask (the reference's `onCommitGroupShare`): modify +
/// move + copy on the group mask, toggled as one.
const GROUP_SHARE_MASK: Permissions = Permissions::MODIFY
    .union(Permissions::MOVE)
    .union(Permissions::COPY);

/// The minimum surviving slice between a cut begin and end (the reference's
/// `OBJECT_MIN_CUT_INC`).
const MIN_CUT_GAP: f32 = 0.02;

/// The maximum hollow, as a fraction (the reference's SL
/// `SL_OBJECT_MAX_HOLLOW_SIZE`, 95%).
const MAX_HOLLOW: f32 = 0.95;

/// The minimum torus-family hole size (the reference's SL
/// `SL_OBJECT_MIN_HOLE_SIZE`).
const MIN_HOLE_SIZE: f32 = 0.05;

/// The maximum torus-family hole size Y (the reference's
/// `OBJECT_MAX_HOLE_SIZE_Y`).
const MAX_HOLE_SIZE_Y: f32 = 0.5;

/// The linear-path twist range, in degrees (`OBJECT_TWIST_LINEAR_MIN/MAX`).
const TWIST_LINEAR_MAX_DEG: f32 = 180.0;

/// The circular-path twist range, in degrees (`OBJECT_TWIST_MIN/MAX`).
const TWIST_CIRCULAR_MAX_DEG: f32 = 360.0;

/// The name field's byte cap (the wire `ObjectName` limit).
const MAX_NAME_CHARS: usize = 63;

/// The description field's byte cap (the wire `ObjectDescription` limit).
const MAX_DESCRIPTION_CHARS: usize = 127;

/// The light-radius cap, in metres (the reference's `LIGHT_MAX_RADIUS` = 20).
const MAX_LIGHT_RADIUS: f32 = 20.0;

/// The light-falloff cap (the reference's `LIGHT_MAX_FALLOFF` = 2).
const MAX_LIGHT_FALLOFF: f32 = 2.0;

/// The spotlight field-of-view cap (the reference's `Light FOV` spinner max).
const MAX_SPOT_FOV: f32 = 3.0;

/// The spotlight focus range (the reference's `Light Focus` spinner ±20).
const MAX_SPOT_FOCUS: f32 = 20.0;

/// The flexi scalar range cap (gravity / friction / wind / tension / force
/// spinners all sit inside ±10 in the reference).
const MAX_FLEXI_SCALAR: f32 = 10.0;

/// The defaults the reference seeds a **newly enabled** flexible path with
/// (the `floater_tools.xml` init values): softness 2, gravity 0.3, drag 2,
/// wind 0, tension 1, zero force.
const FLEXI_DEFAULTS: FlexibleData = FlexibleData {
    softness: 2,
    tension: 1.0,
    air_friction: 2.0,
    gravity: 0.3,
    wind_sensitivity: 0.0,
    user_force: Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
};

/// The defaults the reference's `LLLightParams` constructor seeds a **newly
/// enabled** light with: white at full intensity, radius 10, cutoff 0,
/// falloff 0.75.
const LIGHT_DEFAULTS: LightData = LightData {
    color: [255, 255, 255, 255],
    radius: 10.0,
    cutoff: 0.0,
    falloff: 0.75,
};

// ---------------------------------------------------------------------------
// The prim-type / hollow-shape / material cycles.
// ---------------------------------------------------------------------------

/// The build combo's prim types (the reference's `comboBaseType` entries), plus
/// the read-only sculpt / mesh classifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrimTypeUi {
    /// A box (square profile, line path).
    Box,
    /// A cylinder (circle profile, line path).
    Cylinder,
    /// A prism (triangle profile, line path).
    Prism,
    /// A sphere (half-circle profile, circle path).
    Sphere,
    /// A torus (circle profile, circle path).
    Torus,
    /// A tube (square profile, circle path).
    Tube,
    /// A ring (triangle profile, circle path).
    Ring,
    /// A sculpted prim (a `PARAMS_SCULPT` block with a sculpt-map texture);
    /// not cycled to or from here (the sculpt texture needs the texture
    /// picker).
    Sculpt,
    /// A mesh object (a `PARAMS_SCULPT` block typed mesh); its shape is the
    /// asset's, so the shape rows do not apply.
    Mesh,
}

/// The `LL_SCULPT_TYPE_MASK` low bits of a sculpt-type byte.
const SCULPT_TYPE_MASK: u8 = 0x07;

/// The `LL_SCULPT_TYPE_MESH` stitching type.
const SCULPT_TYPE_MESH: u8 = 5;

impl PrimTypeUi {
    /// Classify a prim's current shape the way the reference's `getState`
    /// does (`llpanelobject.cpp`): sculpt / mesh from the sculpt block first,
    /// then the path × profile combination — with the circle-path
    /// circle-profile split into sphere vs torus by the dequantized
    /// `path_scale_y` (> 0.75 reads as a squashed sphere).
    pub(crate) fn classify(shape: &PrimShapeFloat, sculpt_type: Option<u8>) -> Self {
        if let Some(sculpt_type) = sculpt_type {
            return if sculpt_type & SCULPT_TYPE_MASK == SCULPT_TYPE_MESH {
                Self::Mesh
            } else {
                Self::Sculpt
            };
        }
        match shape.path_curve {
            PathCurve::Line | PathCurve::Flexible => match shape.profile_curve {
                ProfileCurve::Circle => Self::Cylinder,
                ProfileCurve::IsoTriangle
                | ProfileCurve::EqualTriangle
                | ProfileCurve::RightTriangle => Self::Prism,
                ProfileCurve::Square | ProfileCurve::HalfCircle => Self::Box,
            },
            PathCurve::Circle => match shape.profile_curve {
                ProfileCurve::Circle => {
                    if shape.path_scale_y > 0.75 {
                        Self::Sphere
                    } else {
                        Self::Torus
                    }
                }
                ProfileCurve::HalfCircle => Self::Sphere,
                ProfileCurve::EqualTriangle
                | ProfileCurve::IsoTriangle
                | ProfileCurve::RightTriangle => Self::Ring,
                ProfileCurve::Square => Self::Tube,
            },
            PathCurve::Circle2 => Self::Sphere,
        }
    }

    /// The next type in the cycle button's order (the reference combo's
    /// order); the non-cycleable sculpt / mesh types restart at box.
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Box => Self::Cylinder,
            Self::Cylinder => Self::Prism,
            Self::Prism => Self::Sphere,
            Self::Sphere => Self::Torus,
            Self::Torus => Self::Tube,
            Self::Tube => Self::Ring,
            Self::Ring | Self::Sculpt | Self::Mesh => Self::Box,
        }
    }

    /// The profile and path curves the reference's `getVolumeParams` switch
    /// applies when this type is picked (`llpanelobject.cpp:1770-1892`);
    /// `None` for the non-cycleable sculpt / mesh entries.
    pub(crate) const fn curves(self) -> Option<(ProfileCurve, PathCurve)> {
        match self {
            Self::Box => Some((ProfileCurve::Square, PathCurve::Line)),
            Self::Cylinder => Some((ProfileCurve::Circle, PathCurve::Line)),
            Self::Prism => Some((ProfileCurve::EqualTriangle, PathCurve::Line)),
            Self::Sphere => Some((ProfileCurve::HalfCircle, PathCurve::Circle)),
            Self::Torus => Some((ProfileCurve::Circle, PathCurve::Circle)),
            Self::Tube => Some((ProfileCurve::Square, PathCurve::Circle)),
            Self::Ring => Some((ProfileCurve::EqualTriangle, PathCurve::Circle)),
            Self::Sculpt | Self::Mesh => None,
        }
    }

    /// Whether the "Path Cut" row edits the **path** begin/end and the
    /// advanced row the **profile** begin/end (the reference's S/T flip for
    /// sphere / torus / tube / ring); box / cylinder / prism are reversed.
    pub(crate) const fn cut_edits_path(self) -> bool {
        matches!(self, Self::Sphere | Self::Torus | Self::Tube | Self::Ring)
    }

    /// Whether this type's path is circular, giving the ±360° twist range
    /// (a linear path twists ±180°).
    pub(crate) const fn circular_path(self) -> bool {
        matches!(self, Self::Sphere | Self::Torus | Self::Tube | Self::Ring)
    }

    /// Whether the taper-scale row displays `1 − ratio` (the box-family
    /// convention) rather than the raw ratio (sphere / torus-family "hole
    /// size").
    pub(crate) const fn taper_displays_inverted(self) -> bool {
        matches!(self, Self::Box | Self::Cylinder | Self::Prism)
    }

    /// Whether this type shows the torus-family "Hole Size" label on the
    /// scale row (vs the box-family / sphere "Taper").
    const fn size_is_hole(self) -> bool {
        matches!(self, Self::Torus | Self::Tube | Self::Ring)
    }

    /// Whether the shape rows apply at all (a sculpt / mesh keeps its asset's
    /// geometry).
    const fn shape_editable(self) -> bool {
        !matches!(self, Self::Sculpt | Self::Mesh)
    }

    /// The Fluent key naming this type on the cycle button.
    const fn label_key(self) -> &'static str {
        match self {
            Self::Box => "build-type-box",
            Self::Cylinder => "build-type-cylinder",
            Self::Prism => "build-type-prism",
            Self::Sphere => "build-type-sphere",
            Self::Torus => "build-type-torus",
            Self::Tube => "build-type-tube",
            Self::Ring => "build-type-ring",
            Self::Sculpt => "build-type-sculpt",
            Self::Mesh => "build-type-mesh",
        }
    }
}

/// The hollow-shape cycle's next entry (Default → Circle → Square → Triangle).
const fn next_hole_type(hole: HoleType) -> HoleType {
    match hole {
        HoleType::Same => HoleType::Circle,
        HoleType::Circle => HoleType::Square,
        HoleType::Square => HoleType::Triangle,
        HoleType::Triangle => HoleType::Same,
    }
}

/// The Fluent key naming a hollow shape on the cycle button.
const fn hole_label_key(hole: HoleType) -> &'static str {
    match hole {
        HoleType::Same => "build-hole-default",
        HoleType::Circle => "build-hole-circle",
        HoleType::Square => "build-hole-square",
        HoleType::Triangle => "build-hole-triangle",
    }
}

/// The material cycle's next entry — the reference combo's list (stone …
/// rubber, excluding the legacy light material, which cycling leaves).
const fn next_material(material: Material) -> Material {
    match material {
        Material::Stone => Material::Metal,
        Material::Metal => Material::Glass,
        Material::Glass => Material::Wood,
        Material::Wood => Material::Flesh,
        Material::Flesh => Material::Plastic,
        Material::Plastic => Material::Rubber,
        // Rubber wraps; the legacy Light material (not offered by the
        // reference combo either) and any future variant restart at stone.
        _other => Material::Stone,
    }
}

/// The Fluent key naming a material on the cycle button.
const fn material_label_key(material: Material) -> &'static str {
    match material {
        Material::Stone => "build-material-stone",
        Material::Metal => "build-material-metal",
        Material::Glass => "build-material-glass",
        Material::Flesh => "build-material-flesh",
        Material::Plastic => "build-material-plastic",
        Material::Rubber => "build-material-rubber",
        Material::Light => "build-material-light",
        // Wood, and any future variant `Material::from_code` collapses to it.
        _other => "build-material-wood",
    }
}

// ---------------------------------------------------------------------------
// Quantization (the exact inverses of `sl-prim`'s `PrimShape::from_params`).
// ---------------------------------------------------------------------------

/// Quantize a cut **begin** fraction (`[0, 1]` → `begin / 0.00002`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "f32 → u16 at the wire-quantization boundary; the value is clamped to [0, 50000] first"
)]
fn quantize_cut_begin(begin: f32) -> u16 {
    (begin.clamp(0.0, 1.0) * 50000.0).round() as u16
}

/// Quantize a cut **end** fraction (`[0, 1]` → `50000 - end / 0.00002`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "f32 → u16 at the wire-quantization boundary; the value is clamped to [0, 50000] first"
)]
fn quantize_cut_end(end: f32) -> u16 {
    (50000.0 - end.clamp(0.0, 1.0) * 50000.0).round() as u16
}

/// Quantize a hollow fraction (`[0, 0.95]` → `hollow / 0.00002`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "f32 → u16 at the wire-quantization boundary; the value is clamped to [0, 47500] first"
)]
fn quantize_hollow(hollow: f32) -> u16 {
    (hollow.clamp(0.0, MAX_HOLLOW) * 50000.0).round() as u16
}

/// Quantize a path scale ratio (`[0, 2]` → `200 - scale / 0.01`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "f32 → u8 at the wire-quantization boundary; the value is clamped to [0, 200] first"
)]
fn quantize_path_scale(scale: f32) -> u8 {
    (200.0 - scale.clamp(0.0, 2.0) * 100.0).round() as u8
}

/// Quantize a signed centi-unit value (twist / taper / radius offset / skew /
/// shear, quanta `0.01`) into its `i8` wire form.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "f32 → i8 at the wire-quantization boundary; the value is clamped to ±1.27 first"
)]
fn quantize_signed(value: f32) -> i8 {
    (value.clamp(-1.27, 1.27) * 100.0).round() as i8
}

/// Quantize a shear value into the wire's `u8` field, which carries the `i8`
/// two's-complement bits (the viewer reads it back as `S8`).
fn quantize_shear(value: f32) -> u8 {
    quantize_signed(value.clamp(-0.5, 0.5)).cast_unsigned()
}

/// Quantize path revolutions (`[1, 4]` → `(revolutions - 1) / 0.015`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "f32 → u8 at the wire-quantization boundary; the value is clamped to [0, 200] first"
)]
fn quantize_revolutions(revolutions: f32) -> u8 {
    ((revolutions.clamp(1.0, 4.0) - 1.0) / 0.015).round() as u8
}

// ---------------------------------------------------------------------------
// sRGB ↔ linear (the light colour swatch stand-in).
// ---------------------------------------------------------------------------

/// One linear wire colour byte, as its (unrounded) sRGB `0..=255` display
/// value; the field's one-decimal formatting does the rounding.
fn linear_byte_to_srgb(byte: u8) -> f32 {
    let linear = f32::from(byte) / 255.0;
    let srgb = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    srgb * 255.0
}

/// One sRGB `0..=255` display value, converted back to its linear wire byte.
///
/// The whole-number display is coarser than the linear byte grid in the
/// bright range (the sRGB curve compresses there), so after the analytic
/// inverse the neighbouring bytes are checked against the **forward** mapping
/// and the closest taken — an unedited displayed value then always
/// round-trips to the byte it came from.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "f32 → u8 at the wire-colour boundary; the value is clamped to [0, 255] first"
)]
fn srgb_to_linear_byte(srgb_255: f32) -> u8 {
    let srgb_255 = srgb_255.clamp(0.0, 255.0);
    let srgb = srgb_255 / 255.0;
    let linear = if srgb <= 0.040_45 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    };
    let candidate = (linear * 255.0).round().clamp(0.0, 255.0) as u8;
    [
        candidate.saturating_sub(1),
        candidate,
        candidate.saturating_add(1),
    ]
    .into_iter()
    .min_by(|a, b| {
        let error = |byte: u8| (linear_byte_to_srgb(byte) - srgb_255).abs();
        error(*a).total_cmp(&error(*b))
    })
    .unwrap_or(candidate)
}

/// A `[0, 1]` fraction to its wire byte.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "f32 → u8 at the wire-colour boundary; the value is clamped to [0, 255] first"
)]
fn unit_to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

// ---------------------------------------------------------------------------
// The widget markers.
// ---------------------------------------------------------------------------

/// Which editor a parameter text field is, deciding its display value, its
/// clamps, and which commit family an edit belongs to.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ParamField {
    /// The object name (`ObjectName`).
    Name,
    /// The object description (`ObjectDescription`).
    Description,
    /// Path-cut begin (the S/T-flipped "Path Cut" row).
    CutBegin,
    /// Path-cut end.
    CutEnd,
    /// Hollow, displayed as a percentage.
    Hollow,
    /// Twist begin, in degrees.
    TwistBegin,
    /// Twist end, in degrees.
    TwistEnd,
    /// The taper / hole-size X (the wire `path_scale_x`).
    ScaleX,
    /// The taper / hole-size Y (the wire `path_scale_y`).
    ScaleY,
    /// Top shear X.
    ShearX,
    /// Top shear Y.
    ShearY,
    /// The advanced cut begin (the S/T-flipped "Profile Cut" / "Dimple" /
    /// "Slice" row).
    AdvBegin,
    /// The advanced cut end.
    AdvEnd,
    /// The torus-family taper-profile X (the wire `path_taper_x`).
    TaperX,
    /// The torus-family taper-profile Y.
    TaperY,
    /// The path radius offset.
    RadiusOffset,
    /// The path revolutions.
    Revolutions,
    /// The path skew.
    Skew,
    /// Flexi softness (0–3, integer).
    FlexSoftness,
    /// Flexi gravity.
    FlexGravity,
    /// Flexi drag (air friction).
    FlexFriction,
    /// Flexi wind sensitivity.
    FlexWind,
    /// Flexi tension.
    FlexTension,
    /// Flexi force X.
    FlexForceX,
    /// Flexi force Y.
    FlexForceY,
    /// Flexi force Z.
    FlexForceZ,
    /// Light colour red, sRGB `0..=255`.
    LightRed,
    /// Light colour green.
    LightGreen,
    /// Light colour blue.
    LightBlue,
    /// Light intensity, `[0, 1]`.
    LightIntensity,
    /// Light radius, metres.
    LightRadius,
    /// Light falloff.
    LightFalloff,
    /// Spotlight field of view (an existing projector's `LightImage`).
    SpotFov,
    /// Spotlight focus.
    SpotFocus,
    /// Spotlight ambiance.
    SpotAmbiance,
}

/// Which wire message a committed [`ParamField`] edit rebuilds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitFamily {
    /// `ObjectName`.
    Name,
    /// `ObjectDescription`.
    Description,
    /// `ObjectShape` (rebuilt from all shape fields).
    Shape,
    /// `ObjectExtraParams` with the flexi block rebuilt.
    Flexi,
    /// `ObjectExtraParams` with the light block rebuilt.
    Light,
    /// `ObjectExtraParams` with the spotlight projection rebuilt.
    Spot,
}

impl ParamField {
    /// The field's input kind.
    const fn input_kind(self) -> TextInputKind {
        match self {
            Self::Name | Self::Description => TextInputKind::Line,
            Self::FlexSoftness => TextInputKind::NonNegativeInteger,
            _numeric => TextInputKind::Float,
        }
    }

    /// The commit family a committed edit of this field belongs to.
    const fn family(self) -> CommitFamily {
        match self {
            Self::Name => CommitFamily::Name,
            Self::Description => CommitFamily::Description,
            Self::CutBegin
            | Self::CutEnd
            | Self::Hollow
            | Self::TwistBegin
            | Self::TwistEnd
            | Self::ScaleX
            | Self::ScaleY
            | Self::ShearX
            | Self::ShearY
            | Self::AdvBegin
            | Self::AdvEnd
            | Self::TaperX
            | Self::TaperY
            | Self::RadiusOffset
            | Self::Revolutions
            | Self::Skew => CommitFamily::Shape,
            Self::FlexSoftness
            | Self::FlexGravity
            | Self::FlexFriction
            | Self::FlexWind
            | Self::FlexTension
            | Self::FlexForceX
            | Self::FlexForceY
            | Self::FlexForceZ => CommitFamily::Flexi,
            Self::LightRed
            | Self::LightGreen
            | Self::LightBlue
            | Self::LightIntensity
            | Self::LightRadius
            | Self::LightFalloff => CommitFamily::Light,
            Self::SpotFov | Self::SpotFocus | Self::SpotAmbiance => CommitFamily::Spot,
        }
    }

    /// How many decimals the field displays.
    const fn decimals(self) -> usize {
        match self {
            Self::Name
            | Self::Description
            | Self::FlexSoftness
            | Self::TwistBegin
            | Self::TwistEnd => 0,
            // One decimal keeps the sRGB colour display injective over the
            // linear wire bytes (the curve's minimum slope is ≈ 0.44 per
            // byte), so an unedited channel round-trips exactly.
            Self::Hollow | Self::LightRed | Self::LightGreen | Self::LightBlue => 1,
            Self::Revolutions
            | Self::Skew
            | Self::FlexGravity
            | Self::FlexFriction
            | Self::FlexWind
            | Self::FlexTension => 2,
            _fine => 3,
        }
    }

    /// Format `value` the way this field displays it.
    fn format(self, value: f32) -> String {
        format!("{value:.precision$}", precision = self.decimals())
    }

    /// The gate guarding this field.
    const fn gate(self) -> ParamGate {
        match self.family() {
            CommitFamily::Name | CommitFamily::Description => ParamGate::Selection,
            CommitFamily::Shape => ParamGate::ShapeEditable,
            CommitFamily::Flexi => ParamGate::FlexiFields,
            CommitFamily::Light => ParamGate::LightFields,
            CommitFamily::Spot => ParamGate::SpotFields,
        }
    }
}

/// Which build-floater toggle a toggle row flips (distinct from the tool
/// toggles in [`crate::edit_tool`] — these commit wire edits).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ParamToggle {
    /// The `UsePhysics` flag.
    Physical,
    /// The `IsTemporary` flag.
    Temporary,
    /// The `IsPhantom` flag.
    Phantom,
    /// The flexible-path feature.
    Flexi,
    /// The light feature.
    Light,
    /// Next owner can modify (`ObjectPermissions`, next-owner mask).
    NextModify,
    /// Next owner can copy.
    NextCopy,
    /// Next owner can transfer.
    NextTransfer,
    /// Share with group (the group mask's modify + move + copy bits, the
    /// reference's `onCommitGroupShare`).
    ShareGroup,
    /// Anyone can move (the everyone mask's move bit).
    AnyoneMove,
    /// Anyone can copy (the everyone mask's copy bit).
    AnyoneCopy,
}

/// Marks a [`ParamToggle`] row's check glyph.
#[derive(Component, Debug, Clone, Copy)]
struct ParamToggleGlyph(ParamToggle);

/// Which cycle button this is (the combo stand-ins).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ParamCycle {
    /// The prim base type.
    PrimType,
    /// The hollow shape.
    HoleType,
    /// The physical material.
    Material,
    /// The group the object is set to (cycles the agent's own groups; the
    /// reference opens a group picker instead).
    Group,
}

/// Marks a [`ParamCycle`] button's value text.
#[derive(Component, Debug, Clone, Copy)]
struct ParamCycleValue(ParamCycle);

/// Which per-type row container this is, for the visibility pass.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ShapeRow {
    /// The prim-type row (hidden for a non-prim selection only).
    Kind,
    /// The path-cut row.
    Cut,
    /// The hollow + hollow-shape row.
    Hollow,
    /// The twist row.
    Twist,
    /// The taper / hole-size row.
    Scale,
    /// The top-shear row.
    Shear,
    /// The advanced-cut row.
    Advanced,
    /// The torus-family taper-profile row.
    Taper,
    /// The radius offset / revolutions / skew row.
    Circular,
}

/// Which swappable label variant this is (the rows the reference relabels per
/// prim type).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum SwapLabel {
    /// The scale row's box-family / sphere "Taper".
    ScaleTaper,
    /// The scale row's torus-family "Hole Size".
    ScaleHole,
    /// The advanced row's torus-family "Profile Cut".
    AdvProfileCut,
    /// The advanced row's sphere "Dimple".
    AdvDimple,
    /// The advanced row's box-family "Slice".
    AdvSlice,
}

/// The feature sub-sections (always visible; their gates grey the fields out
/// while the feature is off, the reference's cleared-and-disabled look).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum FeatureRows {
    /// The flexible-path parameter rows.
    Flexi,
    /// The light parameter rows.
    Light,
    /// The spotlight projection row.
    Spot,
}

/// Marks a [`ParamToggle`] row's text label (greyed with its gate).
#[derive(Component, Debug, Clone, Copy)]
struct ParamToggleLabel(ParamToggle);

/// What must be true of the selection for an interactive widget to be live.
/// A gated-off widget stays **visible** but greys out and ignores input —
/// the reference viewer's no-selection behaviour (`getState` disables, it
/// does not hide; only the per-type rows hide).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ParamGate {
    /// Any selected, tracked object (name / description, the flag toggles,
    /// the material cycle).
    Selection,
    /// A selected plain prim whose shape is editable — not a sculpt / mesh
    /// (the shape fields and the type / hollow-shape cycles).
    ShapeEditable,
    /// A selected plain prim, sculpts included (the light toggle).
    Prim,
    /// A selected shape-editable prim on a linear or already-flexible path
    /// (the flexi toggle, the reference's `canBeFlexible`).
    FlexiToggle,
    /// The flexible-path feature enabled (its parameter fields).
    FlexiFields,
    /// The light feature enabled (its parameter fields).
    LightFields,
    /// A spotlight projection present (its parameter fields).
    SpotFields,
    /// A selection whose object is set to a group (the Deed button).
    Deed,
}

/// The gate guarding a toggle row.
const fn toggle_gate(toggle: ParamToggle) -> ParamGate {
    match toggle {
        ParamToggle::Physical
        | ParamToggle::Temporary
        | ParamToggle::Phantom
        | ParamToggle::NextModify
        | ParamToggle::NextCopy
        | ParamToggle::NextTransfer
        | ParamToggle::ShareGroup
        | ParamToggle::AnyoneMove
        | ParamToggle::AnyoneCopy => ParamGate::Selection,
        ParamToggle::Flexi => ParamGate::FlexiToggle,
        ParamToggle::Light => ParamGate::Prim,
    }
}

/// The gate guarding a cycle button.
const fn cycle_gate(cycle: ParamCycle) -> ParamGate {
    match cycle {
        ParamCycle::PrimType | ParamCycle::HoleType => ParamGate::ShapeEditable,
        ParamCycle::Material | ParamCycle::Group => ParamGate::Selection,
    }
}

/// The read-only info lines on the General tab (the reference's
/// `llpanelpermissions` labels).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum InfoText {
    /// The creator's resolved name.
    Creator,
    /// The owner's resolved name (an agent, or a group for a deeded object).
    Owner,
    /// What the agent can do with the object, from the update flags' agent-
    /// relative permission bits.
    YouCan,
}

/// A one-shot action button on the parameter tabs.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ParamAction {
    /// Deed the object to the group it is set to (`ObjectOwner`).
    Deed,
}

/// The skin class greying a gated-off widget's text
/// (`--text-disabled`-driven; see `assets/skins/common.css`).
const DISABLED_CLASS: &str = "sk-build-disabled";

/// What a cycle button shows when there is nothing to show a value for.
const NO_VALUE: &str = "—";

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// The tab indices the parameter widgets start at (the shell's transform
/// fields end at 29).
const PARAM_TAB_INDEX: i32 = 30;

/// Spawn a parameter text field with its marker.
fn spawn_param_field(
    commands: &mut Commands,
    parent: Entity,
    field: ParamField,
    element: &'static str,
    width_glyphs: f32,
    tab_index: &mut i32,
) -> Entity {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let entity = spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            font_size: TOOL_FONT_SIZE,
            width_glyphs,
            tab_index: index,
            max_characters: match field {
                ParamField::Name => Some(MAX_NAME_CHARS),
                ParamField::Description => Some(MAX_DESCRIPTION_CHARS),
                _numeric => None,
            },
            ..TextInputSpec::new(element, field.input_kind())
        },
    );
    commands.entity(entity).insert((field, field.gate()));
    entity
}

/// Spawn a labelled **toggle section**: the label on its own line above a
/// non-wrapping row the caller's toggles go into. Used where a wrapping
/// label + toggles row would overlap its second line (the ui-text measure
/// quirk).
fn spawn_toggle_section(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
) -> Entity {
    let section = commands
        .spawn((
            Node {
                ..column(Val::Px(2.0))
            },
            ChildOf(parent),
        ))
        .id();
    spawn_row_label(commands, section, label_key);
    commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(10.0))
            },
            ChildOf(section),
        ))
        .id()
}

/// Spawn a labelled row container under `parent` and return it.
fn spawn_param_row(commands: &mut Commands, parent: Entity, label_key: &'static str) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(4.0))
            },
            ChildOf(parent),
        ))
        .id();
    spawn_row_label(commands, row_entity, label_key);
    row_entity
}

/// Spawn one wire-committing toggle row (check glyph + label).
fn spawn_param_toggle(
    commands: &mut Commands,
    parent: Entity,
    toggle: ParamToggle,
    label_key: &'static str,
    tab_index: &mut i32,
) {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let toggle_row = commands
        .spawn((
            bevy::ui_widgets::Button,
            bevy::input_focus::tab_navigation::TabIndex(index),
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Pickable::default(),
            toggle,
            toggle_gate(toggle),
            Name::new(format!("build-params:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(UNCHECKED_GLYPH),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        ParamToggleGlyph(toggle),
        Pickable::IGNORE,
        ChildOf(toggle_row),
    ));
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        ClassList::new_with_classes([LABEL_CLASS]),
        ParamToggleLabel(toggle),
        Pickable::IGNORE,
        ChildOf(toggle_row),
    ));
    commands.entity(toggle_row).observe(handle_toggle_press);
}

/// Spawn one cycle button (the combo stand-in): a bordered button whose value
/// text the sync pass rewrites.
fn spawn_param_cycle(
    commands: &mut Commands,
    parent: Entity,
    cycle: ParamCycle,
    tab_index: &mut i32,
) {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let button = commands
        .spawn((
            bevy::ui_widgets::Button,
            bevy::input_focus::tab_navigation::TabIndex(index),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..row(Val::ZERO)
            },
            BorderColor::all(Color::srgba(0.4, 0.4, 0.45, 1.0)),
            BackgroundColor(Color::srgba(0.18, 0.18, 0.2, 1.0)),
            cycle,
            cycle_gate(cycle),
            Pickable::default(),
            Name::new(format!("build-params:cycle:{cycle:?}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        ParamCycleValue(cycle),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.entity(button).observe(handle_cycle_press);
}

/// Spawn one action button (a bordered button with a translated label).
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    action: ParamAction,
    label_key: &'static str,
    tab_index: &mut i32,
) {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let button = commands
        .spawn((
            bevy::ui_widgets::Button,
            bevy::input_focus::tab_navigation::TabIndex(index),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..row(Val::ZERO)
            },
            BorderColor::all(Color::srgba(0.4, 0.4, 0.45, 1.0)),
            BackgroundColor(Color::srgba(0.18, 0.18, 0.2, 1.0)),
            action,
            match action {
                ParamAction::Deed => ParamGate::Deed,
            },
            Pickable::default(),
            Name::new(format!("build-params:action:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.entity(button).observe(handle_action_press);
}

/// Spawn one read-only info row: a translated label plus the value text the
/// sync pass rewrites.
fn spawn_info_row(
    commands: &mut Commands,
    parent: Entity,
    info: InfoText,
    label_key: &'static str,
) {
    let info_row = spawn_param_row(commands, parent, label_key);
    commands.spawn((
        Text::default(),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        info,
        ChildOf(info_row),
    ));
}

/// Spawn a swappable row label (one of the per-type variants), wrapped in its
/// own show/hide container.
fn spawn_swap_label(
    commands: &mut Commands,
    parent: Entity,
    variant: SwapLabel,
    label_key: &'static str,
    shown: bool,
) {
    let holder = commands
        .spawn((
            Node {
                min_width: Val::Px(64.0),
                ..Default::default()
            },
            UiPanelShown(shown),
            variant,
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        ClassList::new_with_classes([LABEL_CLASS]),
        ChildOf(holder),
    ));
}

/// Spawn the Object-tab parameter editors (below the shell's transform rows)
/// and the Features-tab editors, into the pages published by
/// [`crate::edit_tool::spawn_build_floater`].
pub(crate) fn spawn_param_tabs(mut commands: Commands, pages: Option<Res<BuildTabPages>>) {
    let Some(pages) = pages else {
        return;
    };
    let mut tab_index = PARAM_TAB_INDEX;

    // ---- General tab ------------------------------------------------------
    // Name / description sit on the reference's General tab
    // (`llpanelpermissions`); its permission / sale surfaces are their own
    // tasks.
    let general = pages.general;

    let name_row = spawn_param_row(&mut commands, general, "build-object-name-label");
    spawn_param_field(
        &mut commands,
        name_row,
        ParamField::Name,
        "build-name",
        18.0,
        &mut tab_index,
    );
    let desc_row = spawn_param_row(&mut commands, general, "build-object-desc-label");
    spawn_param_field(
        &mut commands,
        desc_row,
        ParamField::Description,
        "build-desc",
        18.0,
        &mut tab_index,
    );

    // The read-only info lines (creator / owner / what the agent may do).
    for (info, key) in [
        (InfoText::Creator, "build-info-creator"),
        (InfoText::Owner, "build-info-owner"),
        (InfoText::YouCan, "build-info-you-can"),
    ] {
        spawn_info_row(&mut commands, general, info, key);
    }

    // The group row: the set-group cycle plus the Deed button.
    let group_row = spawn_param_row(&mut commands, general, "build-group-label");
    spawn_param_cycle(&mut commands, group_row, ParamCycle::Group, &mut tab_index);
    spawn_action_button(
        &mut commands,
        group_row,
        ParamAction::Deed,
        "build-deed",
        &mut tab_index,
    );
    spawn_param_toggle(
        &mut commands,
        general,
        ParamToggle::ShareGroup,
        "build-share-group",
        &mut tab_index,
    );

    // Next owner can: modify / copy / transfer. The label sits on its own
    // line above a **non-wrapping** toggle row: a label + toggles row that
    // wraps mid-row overlaps its second line (the ui-text measure quirk), so
    // these sections avoid wrapping altogether — the toggles alone fit the
    // floater's minimum width.
    let next_owner_row = spawn_toggle_section(&mut commands, general, "build-next-owner-label");
    for (toggle, key) in [
        (ParamToggle::NextModify, "build-perm-modify"),
        (ParamToggle::NextCopy, "build-perm-copy"),
        (ParamToggle::NextTransfer, "build-perm-transfer"),
    ] {
        spawn_param_toggle(&mut commands, next_owner_row, toggle, key, &mut tab_index);
    }

    // Anyone can: move / copy.
    let anyone_row = spawn_toggle_section(&mut commands, general, "build-anyone-label");
    for (toggle, key) in [
        (ParamToggle::AnyoneMove, "build-perm-move"),
        (ParamToggle::AnyoneCopy, "build-perm-copy"),
    ] {
        spawn_param_toggle(&mut commands, anyone_row, toggle, key, &mut tab_index);
    }

    // ---- Object tab -------------------------------------------------------
    let object = pages.object;

    // The flag toggles, one wrapping row.
    let flags_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(10.0))
            },
            ChildOf(object),
        ))
        .id();
    for (toggle, key) in [
        (ParamToggle::Physical, "build-flag-physical"),
        (ParamToggle::Temporary, "build-flag-temporary"),
        (ParamToggle::Phantom, "build-flag-phantom"),
    ] {
        spawn_param_toggle(&mut commands, flags_row, toggle, key, &mut tab_index);
    }

    // The prim-type row.
    let kind_row = spawn_param_row(&mut commands, object, "build-type-label");
    commands
        .entity(kind_row)
        .insert((ShapeRow::Kind, UiPanelShown(true)));
    spawn_param_cycle(
        &mut commands,
        kind_row,
        ParamCycle::PrimType,
        &mut tab_index,
    );

    // The shape rows.
    let cut_row = spawn_param_row(&mut commands, object, "build-cut-label");
    commands
        .entity(cut_row)
        .insert((ShapeRow::Cut, UiPanelShown(true)));
    spawn_param_field(
        &mut commands,
        cut_row,
        ParamField::CutBegin,
        "build-cut-begin",
        7.0,
        &mut tab_index,
    );
    spawn_param_field(
        &mut commands,
        cut_row,
        ParamField::CutEnd,
        "build-cut-end",
        7.0,
        &mut tab_index,
    );

    let hollow_row = spawn_param_row(&mut commands, object, "build-hollow-label");
    commands
        .entity(hollow_row)
        .insert((ShapeRow::Hollow, UiPanelShown(true)));
    spawn_param_field(
        &mut commands,
        hollow_row,
        ParamField::Hollow,
        "build-hollow",
        6.0,
        &mut tab_index,
    );
    spawn_param_cycle(
        &mut commands,
        hollow_row,
        ParamCycle::HoleType,
        &mut tab_index,
    );

    let twist_row = spawn_param_row(&mut commands, object, "build-twist-label");
    commands
        .entity(twist_row)
        .insert((ShapeRow::Twist, UiPanelShown(true)));
    spawn_param_field(
        &mut commands,
        twist_row,
        ParamField::TwistBegin,
        "build-twist-begin",
        6.0,
        &mut tab_index,
    );
    spawn_param_field(
        &mut commands,
        twist_row,
        ParamField::TwistEnd,
        "build-twist-end",
        6.0,
        &mut tab_index,
    );

    // The taper / hole-size row carries both swappable labels.
    let scale_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(4.0))
            },
            ShapeRow::Scale,
            UiPanelShown(true),
            ChildOf(object),
        ))
        .id();
    spawn_swap_label(
        &mut commands,
        scale_row,
        SwapLabel::ScaleTaper,
        "build-taper-label",
        true,
    );
    spawn_swap_label(
        &mut commands,
        scale_row,
        SwapLabel::ScaleHole,
        "build-hole-size-label",
        false,
    );
    spawn_param_field(
        &mut commands,
        scale_row,
        ParamField::ScaleX,
        "build-scale-x",
        7.0,
        &mut tab_index,
    );
    spawn_param_field(
        &mut commands,
        scale_row,
        ParamField::ScaleY,
        "build-scale-y",
        7.0,
        &mut tab_index,
    );

    let shear_row = spawn_param_row(&mut commands, object, "build-shear-label");
    commands
        .entity(shear_row)
        .insert((ShapeRow::Shear, UiPanelShown(true)));
    spawn_param_field(
        &mut commands,
        shear_row,
        ParamField::ShearX,
        "build-shear-x",
        7.0,
        &mut tab_index,
    );
    spawn_param_field(
        &mut commands,
        shear_row,
        ParamField::ShearY,
        "build-shear-y",
        7.0,
        &mut tab_index,
    );

    // The advanced-cut row carries its three swappable labels.
    let adv_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(4.0))
            },
            ShapeRow::Advanced,
            UiPanelShown(true),
            ChildOf(object),
        ))
        .id();
    spawn_swap_label(
        &mut commands,
        adv_row,
        SwapLabel::AdvProfileCut,
        "build-adv-profile-cut-label",
        false,
    );
    spawn_swap_label(
        &mut commands,
        adv_row,
        SwapLabel::AdvDimple,
        "build-adv-dimple-label",
        false,
    );
    spawn_swap_label(
        &mut commands,
        adv_row,
        SwapLabel::AdvSlice,
        "build-adv-slice-label",
        true,
    );
    spawn_param_field(
        &mut commands,
        adv_row,
        ParamField::AdvBegin,
        "build-adv-begin",
        7.0,
        &mut tab_index,
    );
    spawn_param_field(
        &mut commands,
        adv_row,
        ParamField::AdvEnd,
        "build-adv-end",
        7.0,
        &mut tab_index,
    );

    let taper_row = spawn_param_row(&mut commands, object, "build-taper2-label");
    commands
        .entity(taper_row)
        .insert((ShapeRow::Taper, UiPanelShown(true)));
    spawn_param_field(
        &mut commands,
        taper_row,
        ParamField::TaperX,
        "build-taper-x",
        7.0,
        &mut tab_index,
    );
    spawn_param_field(
        &mut commands,
        taper_row,
        ParamField::TaperY,
        "build-taper-y",
        7.0,
        &mut tab_index,
    );

    // Radius offset / revolutions / skew, one wrapping row of labelled pairs.
    let circular_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(8.0))
            },
            ShapeRow::Circular,
            UiPanelShown(true),
            ChildOf(object),
        ))
        .id();
    for (field, key, element) in [
        (
            ParamField::RadiusOffset,
            "build-radius-offset-label",
            "build-radius-offset",
        ),
        (
            ParamField::Revolutions,
            "build-revolutions-label",
            "build-revolutions",
        ),
        (ParamField::Skew, "build-skew-label", "build-skew"),
    ] {
        let pair = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    ..row(Val::Px(4.0))
                },
                ChildOf(circular_row),
            ))
            .id();
        spawn_row_label(&mut commands, pair, key);
        spawn_param_field(&mut commands, pair, field, element, 6.0, &mut tab_index);
    }

    // ---- Features tab ------------------------------------------------------
    let features = pages.features;

    let material_row = spawn_param_row(&mut commands, features, "build-material-label");
    spawn_param_cycle(
        &mut commands,
        material_row,
        ParamCycle::Material,
        &mut tab_index,
    );

    spawn_param_toggle(
        &mut commands,
        features,
        ParamToggle::Flexi,
        "build-feature-flexi",
        &mut tab_index,
    );
    let flexi_rows = commands
        .spawn((
            Node {
                padding: UiRect::left(Val::Px(14.0)),
                ..column(Val::Px(4.0))
            },
            FeatureRows::Flexi,
            UiPanelShown(true),
            ChildOf(features),
        ))
        .id();
    for (field, key, element) in [
        (
            ParamField::FlexSoftness,
            "build-flex-softness-label",
            "build-flex-softness",
        ),
        (
            ParamField::FlexGravity,
            "build-flex-gravity-label",
            "build-flex-gravity",
        ),
        (
            ParamField::FlexFriction,
            "build-flex-friction-label",
            "build-flex-friction",
        ),
        (
            ParamField::FlexWind,
            "build-flex-wind-label",
            "build-flex-wind",
        ),
        (
            ParamField::FlexTension,
            "build-flex-tension-label",
            "build-flex-tension",
        ),
    ] {
        let field_row = spawn_param_row(&mut commands, flexi_rows, key);
        spawn_param_field(
            &mut commands,
            field_row,
            field,
            element,
            6.0,
            &mut tab_index,
        );
    }
    let force_row = spawn_param_row(&mut commands, flexi_rows, "build-flex-force-label");
    for (field, element) in [
        (ParamField::FlexForceX, "build-flex-force-x"),
        (ParamField::FlexForceY, "build-flex-force-y"),
        (ParamField::FlexForceZ, "build-flex-force-z"),
    ] {
        spawn_param_field(
            &mut commands,
            force_row,
            field,
            element,
            6.0,
            &mut tab_index,
        );
    }

    spawn_param_toggle(
        &mut commands,
        features,
        ParamToggle::Light,
        "build-feature-light",
        &mut tab_index,
    );
    let light_rows = commands
        .spawn((
            Node {
                padding: UiRect::left(Val::Px(14.0)),
                ..column(Val::Px(4.0))
            },
            FeatureRows::Light,
            UiPanelShown(true),
            ChildOf(features),
        ))
        .id();
    let color_row = spawn_param_row(&mut commands, light_rows, "build-light-color-label");
    for (field, element) in [
        (ParamField::LightRed, "build-light-red"),
        (ParamField::LightGreen, "build-light-green"),
        (ParamField::LightBlue, "build-light-blue"),
    ] {
        spawn_param_field(
            &mut commands,
            color_row,
            field,
            element,
            5.0,
            &mut tab_index,
        );
    }
    for (field, key, element) in [
        (
            ParamField::LightIntensity,
            "build-light-intensity-label",
            "build-light-intensity",
        ),
        (
            ParamField::LightRadius,
            "build-light-radius-label",
            "build-light-radius",
        ),
        (
            ParamField::LightFalloff,
            "build-light-falloff-label",
            "build-light-falloff",
        ),
    ] {
        let field_row = spawn_param_row(&mut commands, light_rows, key);
        spawn_param_field(
            &mut commands,
            field_row,
            field,
            element,
            6.0,
            &mut tab_index,
        );
    }
    let spot_row = spawn_param_row(&mut commands, light_rows, "build-spot-label");
    commands
        .entity(spot_row)
        .insert((FeatureRows::Spot, UiPanelShown(true)));
    for (field, element) in [
        (ParamField::SpotFov, "build-spot-fov"),
        (ParamField::SpotFocus, "build-spot-focus"),
        (ParamField::SpotAmbiance, "build-spot-ambiance"),
    ] {
        spawn_param_field(&mut commands, spot_row, field, element, 6.0, &mut tab_index);
    }
}

// ---------------------------------------------------------------------------
// Reading the selection into display values.
// ---------------------------------------------------------------------------

/// The display value of one shape field for the current dequantized shape
/// under `prim_type`'s conventions (S/T flip, twist scaling, hollow percent,
/// box-family taper inversion).
fn shape_display_value(field: ParamField, shape: &PrimShapeFloat, prim_type: PrimTypeUi) -> f32 {
    let twist_scale = if prim_type.circular_path() {
        TWIST_CIRCULAR_MAX_DEG
    } else {
        TWIST_LINEAR_MAX_DEG
    };
    let cut_is_path = prim_type.cut_edits_path();
    match field {
        ParamField::CutBegin => {
            if cut_is_path {
                shape.path_begin
            } else {
                shape.profile_begin
            }
        }
        ParamField::CutEnd => {
            if cut_is_path {
                shape.path_end
            } else {
                shape.profile_end
            }
        }
        ParamField::AdvBegin => {
            if cut_is_path {
                shape.profile_begin
            } else {
                shape.path_begin
            }
        }
        ParamField::AdvEnd => {
            if cut_is_path {
                shape.profile_end
            } else {
                shape.path_end
            }
        }
        ParamField::Hollow => shape.hollow * 100.0,
        ParamField::TwistBegin => shape.twist_begin * twist_scale,
        ParamField::TwistEnd => shape.twist_end * twist_scale,
        ParamField::ScaleX => {
            if prim_type.taper_displays_inverted() {
                1.0 - shape.path_scale_x
            } else {
                shape.path_scale_x
            }
        }
        ParamField::ScaleY => {
            if prim_type.taper_displays_inverted() {
                1.0 - shape.path_scale_y
            } else {
                shape.path_scale_y
            }
        }
        ParamField::ShearX => shape.path_shear_x,
        ParamField::ShearY => shape.path_shear_y,
        ParamField::TaperX => shape.taper_x,
        ParamField::TaperY => shape.taper_y,
        ParamField::RadiusOffset => shape.radius_offset,
        ParamField::Revolutions => shape.revolutions,
        ParamField::Skew => shape.skew,
        _not_shape => 0.0,
    }
}

/// The display value of one Features-tab numeric field from the object's
/// extra params.
fn feature_display_value(field: ParamField, extra: &ObjectExtraParams) -> f32 {
    match field {
        ParamField::FlexSoftness => extra
            .flexible
            .as_ref()
            .map_or(0.0, |flexi| f32::from(flexi.softness)),
        ParamField::FlexGravity => extra.flexible.as_ref().map_or(0.0, |flexi| flexi.gravity),
        ParamField::FlexFriction => extra
            .flexible
            .as_ref()
            .map_or(0.0, |flexi| flexi.air_friction),
        ParamField::FlexWind => extra
            .flexible
            .as_ref()
            .map_or(0.0, |flexi| flexi.wind_sensitivity),
        ParamField::FlexTension => extra.flexible.as_ref().map_or(0.0, |flexi| flexi.tension),
        ParamField::FlexForceX => extra
            .flexible
            .as_ref()
            .map_or(0.0, |flexi| flexi.user_force.x),
        ParamField::FlexForceY => extra
            .flexible
            .as_ref()
            .map_or(0.0, |flexi| flexi.user_force.y),
        ParamField::FlexForceZ => extra
            .flexible
            .as_ref()
            .map_or(0.0, |flexi| flexi.user_force.z),
        ParamField::LightRed => extra
            .light
            .map_or(0.0, |light| linear_byte_to_srgb(light.color[0])),
        ParamField::LightGreen => extra
            .light
            .map_or(0.0, |light| linear_byte_to_srgb(light.color[1])),
        ParamField::LightBlue => extra
            .light
            .map_or(0.0, |light| linear_byte_to_srgb(light.color[2])),
        ParamField::LightIntensity => extra
            .light
            .map_or(0.0, |light| f32::from(light.color[3]) / 255.0),
        ParamField::LightRadius => extra.light.map_or(0.0, |light| light.radius),
        ParamField::LightFalloff => extra.light.map_or(0.0, |light| light.falloff),
        ParamField::SpotFov => extra
            .light_image
            .as_ref()
            .map_or(0.0, |image| image.params.x),
        ParamField::SpotFocus => extra
            .light_image
            .as_ref()
            .map_or(0.0, |image| image.params.y),
        ParamField::SpotAmbiance => extra
            .light_image
            .as_ref()
            .map_or(0.0, |image| image.params.z),
        _not_feature => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Building the wire messages back from the displayed fields.
// ---------------------------------------------------------------------------

/// The parsed display values of every shape field, as committed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShapeUiValues {
    /// The "Path Cut" row begin/end.
    pub(crate) cut_begin: f32,
    /// See [`cut_begin`](Self::cut_begin).
    pub(crate) cut_end: f32,
    /// Hollow, as the displayed percentage.
    pub(crate) hollow_percent: f32,
    /// Twist begin/end, in displayed degrees.
    pub(crate) twist_begin_deg: f32,
    /// See [`twist_begin_deg`](Self::twist_begin_deg).
    pub(crate) twist_end_deg: f32,
    /// The taper / hole-size row, as displayed.
    pub(crate) scale_x: f32,
    /// See [`scale_x`](Self::scale_x).
    pub(crate) scale_y: f32,
    /// Top shear.
    pub(crate) shear_x: f32,
    /// See [`shear_x`](Self::shear_x).
    pub(crate) shear_y: f32,
    /// The advanced-cut row begin/end.
    pub(crate) adv_begin: f32,
    /// See [`adv_begin`](Self::adv_begin).
    pub(crate) adv_end: f32,
    /// The taper-profile row.
    pub(crate) taper_x: f32,
    /// See [`taper_x`](Self::taper_x).
    pub(crate) taper_y: f32,
    /// The path radius offset.
    pub(crate) radius_offset: f32,
    /// The path revolutions.
    pub(crate) revolutions: f32,
    /// The path skew.
    pub(crate) skew: f32,
}

/// Clamp a cut pair to the reference's rules: each end in `[0, 1]`, and the
/// begin pulled down so at least [`MIN_CUT_GAP`] survives.
fn clamp_cut_pair(begin: f32, end: f32) -> (f32, f32) {
    let end = end.clamp(MIN_CUT_GAP, 1.0);
    let begin = begin.clamp(0.0, 1.0 - MIN_CUT_GAP).min(end - MIN_CUT_GAP);
    (begin.max(0.0), end)
}

/// Build the full quantized [`PrimShapeParams`] from the displayed shape
/// fields — the reference's `getVolumeParams`: interpret the fields under
/// `prim_type`'s conventions (S/T flip, twist scaling, hollow percent), with
/// the box-family taper inversion keyed off `inversion_type` (the *previously
/// shown* type, the reference's `mSelectedType` gotcha), the hole nibble from
/// `hole`, and the path curve kept flexible when the prim is flexi.
pub(crate) fn shape_from_ui(
    ui: &ShapeUiValues,
    prim_type: PrimTypeUi,
    inversion_type: PrimTypeUi,
    hole: HoleType,
    flexible: bool,
) -> Option<PrimShapeParams> {
    let (profile_curve, path_curve) = prim_type.curves()?;
    let path_curve = if path_curve == PathCurve::Line && flexible {
        PathCurve::Flexible
    } else {
        path_curve
    };

    let twist_scale = if prim_type.circular_path() {
        TWIST_CIRCULAR_MAX_DEG
    } else {
        TWIST_LINEAR_MAX_DEG
    };
    let twist_begin = (ui.twist_begin_deg.clamp(-twist_scale, twist_scale)) / twist_scale;
    let twist_end = (ui.twist_end_deg.clamp(-twist_scale, twist_scale)) / twist_scale;

    // The taper-scale row: undo the box-family display inversion (keyed off
    // the previously shown type), then clamp per the new type.
    let mut scale_x = if inversion_type.taper_displays_inverted() {
        1.0 - ui.scale_x
    } else {
        ui.scale_x
    };
    let mut scale_y = if inversion_type.taper_displays_inverted() {
        1.0 - ui.scale_y
    } else {
        ui.scale_y
    };
    match prim_type {
        PrimTypeUi::Sphere => {
            scale_x = scale_x.clamp(0.0, 1.0);
            scale_y = scale_y.clamp(0.0, 1.0);
        }
        PrimTypeUi::Torus | PrimTypeUi::Tube | PrimTypeUi::Ring => {
            scale_x = scale_x.clamp(MIN_HOLE_SIZE, 1.0);
            scale_y = scale_y.clamp(MIN_HOLE_SIZE, MAX_HOLE_SIZE_Y);
        }
        _box_family => {
            scale_x = scale_x.clamp(0.0, 2.0);
            scale_y = scale_y.clamp(0.0, 2.0);
        }
    }

    let (cut_begin, cut_end) = clamp_cut_pair(ui.cut_begin, ui.cut_end);
    let (adv_begin, adv_end) = clamp_cut_pair(ui.adv_begin, ui.adv_end);
    // The S/T flip: which displayed row carries the path vs the profile cut.
    let (path_pair, profile_pair) = if prim_type.cut_edits_path() {
        ((cut_begin, cut_end), (adv_begin, adv_end))
    } else {
        ((adv_begin, adv_end), (cut_begin, cut_end))
    };

    Some(PrimShapeParams {
        path_curve: path_curve.to_byte(),
        profile_curve: profile_curve.to_byte() | hole.to_byte(),
        path_begin: quantize_cut_begin(path_pair.0),
        path_end: quantize_cut_end(path_pair.1),
        path_scale_x: quantize_path_scale(scale_x),
        path_scale_y: quantize_path_scale(scale_y),
        path_shear_x: quantize_shear(ui.shear_x),
        path_shear_y: quantize_shear(ui.shear_y),
        path_twist: quantize_signed(twist_end),
        path_twist_begin: quantize_signed(twist_begin),
        path_radius_offset: quantize_signed(ui.radius_offset.clamp(-1.0, 1.0)),
        path_taper_x: quantize_signed(ui.taper_x.clamp(-1.0, 1.0)),
        path_taper_y: quantize_signed(ui.taper_y.clamp(-1.0, 1.0)),
        path_revolutions: quantize_revolutions(ui.revolutions),
        path_skew: quantize_signed(ui.skew.clamp(-0.95, 0.95)),
        profile_begin: quantize_cut_begin(profile_pair.0),
        profile_end: quantize_cut_end(profile_pair.1),
        profile_hollow: quantize_hollow(ui.hollow_percent.clamp(0.0, 100.0) / 100.0),
    })
}

/// Build the flexi block from the displayed Features-tab fields.
fn flexi_from_ui(values: &dyn Fn(ParamField) -> Option<f32>) -> Option<FlexibleData> {
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "f32 → u8 softness at the field boundary; the value is clamped to [0, 3] first"
    )]
    let softness = values(ParamField::FlexSoftness)?.clamp(0.0, 3.0).round() as u8;
    let scalar = |field: ParamField| -> Option<f32> {
        values(field).map(|value| value.clamp(-MAX_FLEXI_SCALAR, MAX_FLEXI_SCALAR))
    };
    Some(FlexibleData {
        softness,
        tension: scalar(ParamField::FlexTension)?.max(0.0),
        air_friction: scalar(ParamField::FlexFriction)?.max(0.0),
        gravity: scalar(ParamField::FlexGravity)?,
        wind_sensitivity: scalar(ParamField::FlexWind)?.max(0.0),
        user_force: Vector {
            x: scalar(ParamField::FlexForceX)?,
            y: scalar(ParamField::FlexForceY)?,
            z: scalar(ParamField::FlexForceZ)?,
        },
    })
}

/// Build the light block from the displayed Features-tab fields.
fn light_from_ui(
    values: &dyn Fn(ParamField) -> Option<f32>,
    current: LightData,
) -> Option<LightData> {
    Some(LightData {
        color: [
            srgb_to_linear_byte(values(ParamField::LightRed)?),
            srgb_to_linear_byte(values(ParamField::LightGreen)?),
            srgb_to_linear_byte(values(ParamField::LightBlue)?),
            unit_to_byte(values(ParamField::LightIntensity)?),
        ],
        radius: values(ParamField::LightRadius)?.clamp(0.0, MAX_LIGHT_RADIUS),
        cutoff: current.cutoff,
        falloff: values(ParamField::LightFalloff)?.clamp(0.0, MAX_LIGHT_FALLOFF),
    })
}

// ---------------------------------------------------------------------------
// The plugin and its systems.
// ---------------------------------------------------------------------------

/// What the widgets currently display, compared each frame so they rewrite
/// only when the underlying data (or the selection, or the locale) actually
/// changed — a commit's displayed value therefore survives until the
/// simulator's echo lands, instead of flickering back.
#[derive(Resource, Debug, Default)]
struct ShownSnapshot {
    /// The last-displayed state, or `None` when nothing valid is shown yet.
    shown: Option<SnapshotData>,
}

/// See [`ShownSnapshot`].
#[derive(Debug, Clone, PartialEq)]
struct SnapshotData {
    /// The primary selection's scoped id.
    scoped: ScopedObjectId,
    /// The object's name, from its properties reply.
    name: String,
    /// The object's description.
    description: String,
    /// The `PrimFlags` bits.
    update_flags: u32,
    /// The material byte.
    material: u8,
    /// The object class byte.
    pcode: u8,
    /// The quantized shape.
    shape: PrimShapeParams,
    /// The complete extra params.
    extra: ObjectExtraParams,
    /// The creator line, as displayed (resolved name or an ellipsis while the
    /// name request is in flight).
    creator_label: String,
    /// The owner line, as displayed.
    owner_label: String,
    /// The group the object is set to, from its properties reply.
    group: Option<GroupKey>,
    /// The group line, as displayed.
    group_label: String,
    /// The five permission masks, once the properties reply landed.
    permissions: Option<Permissions5>,
}

/// The shape-row visibility query (aliased for clippy's type-complexity cap);
/// the three `UiPanelShown` writers are disjoint by their marker filters.
type ShapeRowQuery<'w, 's> = Query<
    'w,
    's,
    (&'static ShapeRow, &'static mut UiPanelShown),
    (Without<SwapLabel>, Without<FeatureRows>),
>;

/// The swappable-label visibility query. See [`ShapeRowQuery`].
type SwapLabelQuery<'w, 's> = Query<
    'w,
    's,
    (&'static SwapLabel, &'static mut UiPanelShown),
    (Without<ShapeRow>, Without<FeatureRows>),
>;

/// The feature-rows visibility query. See [`ShapeRowQuery`].
type FeatureRowQuery<'w, 's> = Query<
    'w,
    's,
    (&'static FeatureRows, &'static mut UiPanelShown),
    (Without<ShapeRow>, Without<SwapLabel>),
>;

/// Which parameter field held keyboard focus last frame, to commit on blur.
#[derive(Resource, Debug, Default)]
struct ParamFieldFocus {
    /// The field entity focused last frame, if any.
    last: Option<Entity>,
}

/// The plugin wiring the parameter tabs into the viewer. Registered by
/// [`crate::edit_tool::EditToolPlugin`]'s startup chain (the pages must exist
/// first).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EditParamsPlugin;

impl Plugin for EditParamsPlugin {
    /// Register the snapshot / focus state and the sync + commit systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<ShownSnapshot>()
            .init_resource::<ParamFieldFocus>()
            .add_systems(Update, (sync_param_widgets, commit_param_fields).chain());
    }
}

/// An owned copy of one object's editable wire state, so a commit can read it
/// and then mutate [`ObjectState`] (the local echo) without a borrow conflict.
#[derive(Debug, Clone)]
struct OwnedEditData {
    /// The object class byte.
    pcode: u8,
    /// The quantized shape.
    shape: PrimShapeParams,
    /// The material byte.
    material: u8,
    /// The `PrimFlags` bits.
    update_flags: u32,
    /// The complete extra params.
    extra: ObjectExtraParams,
}

/// The owned editable state of `scoped`, or `None` if untracked.
fn owned_edit_data(objects: &ObjectState, scoped: &ScopedObjectId) -> Option<OwnedEditData> {
    objects.edit_data(scoped).map(|data| OwnedEditData {
        pcode: data.pcode,
        shape: data.shape,
        material: data.material,
        update_flags: data.update_flags,
        extra: data.extra.clone(),
    })
}

/// What an unresolved (still-requesting) name displays.
const PENDING_NAME: &str = "…";

/// The display line for an agent: the resolved legacy name, or an ellipsis
/// while the (deduplicated) name request is in flight.
fn agent_label(
    agent: AgentKey,
    avatars: &mut AvatarState,
    names: &mut MessageWriter<SlCommand>,
) -> String {
    avatars.request_name(agent, names);
    avatars
        .name_of(agent)
        .map_or_else(|| PENDING_NAME.to_owned(), str::to_owned)
}

/// The display line for a group: its name when the agent is a member, else
/// its id (the reference resolves any group via its name cache; a group-name
/// request path is not built here yet).
fn group_label(group: GroupKey, groups: &GroupsModel) -> String {
    groups
        .group_name(group)
        .map_or_else(|| group.uuid().to_string(), str::to_owned)
}

/// Build the current [`SnapshotData`] for the primary selection, or `None`
/// when nothing tracked is selected. Resolving the creator / owner names
/// requests them (deduplicated) as a side effect; the arriving reply changes
/// the snapshot, which rewrites the info lines.
fn build_snapshot(
    selection: &SelectionSet,
    objects: &ObjectState,
    avatars: &mut AvatarState,
    groups: &GroupsModel,
    names: &mut MessageWriter<SlCommand>,
) -> Option<SnapshotData> {
    let primary = selection.primary()?;
    let data = owned_edit_data(objects, &primary.scoped)?;
    let properties = primary.properties.as_ref();
    let (name, description) = properties.map_or_else(
        || (String::new(), String::new()),
        |properties| (properties.name.clone(), properties.description.clone()),
    );
    let creator_label = properties.map_or_else(String::new, |properties| {
        agent_label(properties.creator_id, avatars, names)
    });
    let owner_label = properties.map_or_else(String::new, |properties| match properties.owner {
        OwnerKey::Agent(agent) => agent_label(agent, avatars, names),
        OwnerKey::Group(group) => group_label(group, groups),
    });
    let group = properties.and_then(|properties| properties.group);
    Some(SnapshotData {
        scoped: primary.scoped,
        name,
        description,
        update_flags: data.update_flags,
        material: data.material,
        pcode: data.pcode,
        shape: data.shape,
        extra: data.extra,
        creator_label,
        owner_label,
        group,
        group_label: group.map_or_else(String::new, |group| group_label(group, groups)),
        permissions: properties.map(|properties| properties.permissions),
    })
}

/// A toggle glyph's text + class query row. See [`ParamWidgets`].
type GlyphQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static ParamToggleGlyph,
        &'static mut Text,
        &'static mut ClassList,
    ),
    (
        Without<ParamCycleValue>,
        Without<ParamToggleLabel>,
        Without<InfoText>,
    ),
>;

/// A toggle label's class query row. See [`ParamWidgets`].
type ToggleLabelQuery<'w, 's> = Query<
    'w,
    's,
    (&'static ParamToggleLabel, &'static mut ClassList),
    (Without<ParamCycleValue>, Without<ParamToggleGlyph>),
>;

/// A cycle value's text + class query row. See [`ParamWidgets`].
type CycleValueQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static ParamCycleValue,
        &'static mut Text,
        &'static mut ClassList,
    ),
    (
        Without<ParamToggleGlyph>,
        Without<ParamToggleLabel>,
        Without<InfoText>,
    ),
>;

/// A read-only info line's text query row. See [`ParamWidgets`].
type InfoQuery<'w, 's> = Query<
    'w,
    's,
    (&'static InfoText, &'static mut Text),
    (Without<ParamToggleGlyph>, Without<ParamCycleValue>),
>;

/// Add or remove the greyed-out skin class on a widget text.
fn set_disabled_class(class_list: &mut ClassList, disabled: bool) {
    if disabled {
        if !class_list.contains(DISABLED_CLASS) {
            class_list.add(DISABLED_CLASS);
        }
    } else if class_list.contains(DISABLED_CLASS) {
        class_list.remove(DISABLED_CLASS);
    }
}

/// Whether the snapshot's permission mask picked by `mask_of` contains `bit`
/// (false while no selection / no properties reply yet).
fn perm_bit(
    data: Option<&SnapshotData>,
    mask_of: impl Fn(&Permissions5) -> Permissions,
    bit: Permissions,
) -> bool {
    data.and_then(|data| data.permissions.as_ref())
        .is_some_and(|permissions| mask_of(permissions).contains(bit))
}

/// The widget queries the sync pass rewrites, bundled to stay inside Bevy's
/// system-parameter limit. The three `ClassList` writers are disjoint by
/// their marker filters (each text carries exactly one of the markers).
#[derive(bevy::ecs::system::SystemParam)]
struct ParamWidgets<'w, 's> {
    /// The parameter text fields.
    editors: Query<'w, 's, (Entity, &'static ParamField, &'static mut EditableText)>,
    /// The toggle rows' check glyphs.
    glyphs: GlyphQuery<'w, 's>,
    /// The toggle rows' text labels.
    toggle_labels: ToggleLabelQuery<'w, 's>,
    /// The cycle buttons' value texts.
    cycle_values: CycleValueQuery<'w, 's>,
    /// The read-only info lines.
    infos: InfoQuery<'w, 's>,
    /// Every gated interactive widget root.
    gates: Query<'w, 's, (Entity, &'static ParamGate)>,
    /// The per-type shape row containers.
    shape_rows: ShapeRowQuery<'w, 's>,
    /// The swappable row labels.
    swap_labels: SwapLabelQuery<'w, 's>,
    /// The feature sub-sections.
    feature_rows: FeatureRowQuery<'w, 's>,
}

/// Mirror the primary selection into every parameter widget: field texts,
/// toggle glyphs, cycle labels, the per-type row visibility, and the
/// gated enabled / greyed-out state. Rewrites only when the [`ShownSnapshot`]
/// changed (or the localization did), and never touches the focused field.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection / object state, the snapshot, the focus, the bundled widget queries, \
              the translator, and the text-layout contexts a programmatic rewrite needs"
)]
fn sync_param_widgets(
    state: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut avatars: ResMut<AvatarState>,
    groups: Res<GroupsModel>,
    translator: Translator,
    localization: Res<Localization>,
    locale: Res<crate::i18n::UiLocale>,
    mut snapshot: ResMut<ShownSnapshot>,
    focus: Res<InputFocus>,
    mut widgets: ParamWidgets,
    mut commands: Commands,
    mut names: MessageWriter<SlCommand>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    if !state.active {
        return;
    }
    let current = build_snapshot(&selection, &objects, &mut avatars, &groups, &mut names);
    if snapshot.shown.as_ref() == current.as_ref()
        && !localization.is_changed()
        && !locale.is_changed()
    {
        return;
    }

    let data = current.as_ref();
    let has_selection = data.is_some();
    let is_prim = data.is_some_and(|data| data.pcode == pcode::PRIMITIVE);
    let float_shape = data.map(|data| PrimShapeFloat::from_params(&data.shape));
    let prim_type = match (float_shape.as_ref(), data) {
        (Some(shape), Some(data)) if is_prim => Some(PrimTypeUi::classify(
            shape,
            data.extra.sculpt.map(|sculpt| sculpt.sculpt_type),
        )),
        _no_prim => None,
    };
    let shape_editable = prim_type.is_some_and(PrimTypeUi::shape_editable);
    let flexi_on = data.is_some_and(|data| data.extra.flexible.is_some());
    let light_on = data.is_some_and(|data| data.extra.light.is_some());
    let spot_on = light_on && data.is_some_and(|data| data.extra.light_image.is_some());
    let flexi_toggle_ok = shape_editable
        && float_shape
            .as_ref()
            .is_some_and(|shape| matches!(shape.path_curve, PathCurve::Line | PathCurve::Flexible));
    let deed_ok = data.is_some_and(|data| data.group.is_some());
    let enabled_for = |gate: ParamGate| -> bool {
        match gate {
            ParamGate::Selection => has_selection,
            ParamGate::ShapeEditable => shape_editable,
            ParamGate::Prim => is_prim,
            ParamGate::FlexiToggle => flexi_toggle_ok,
            ParamGate::FlexiFields => flexi_on,
            ParamGate::LightFields => light_on,
            ParamGate::SpotFields => spot_on,
            ParamGate::Deed => deed_ok,
        }
    };
    // The gated enabled state: a gated-off widget ignores the pointer (no
    // press, no click-to-focus) and reads as disabled to the headless
    // widgets; its texts grey out below.
    for (entity, gate) in &widgets.gates {
        if enabled_for(*gate) {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>()
                .insert(Pickable::default());
        } else {
            commands
                .entity(entity)
                .insert((bevy::ui::InteractionDisabled, Pickable::IGNORE));
        }
    }

    // Field texts.
    for (entity, field, mut editor) in &mut widgets.editors {
        if focus.get() == Some(entity) {
            continue;
        }
        let want = match (field, data) {
            (ParamField::Name, Some(data)) => data.name.clone(),
            (ParamField::Description, Some(data)) => data.description.clone(),
            (ParamField::Name | ParamField::Description, None) => String::new(),
            (_numeric, None) => String::new(),
            (field, Some(data)) => match field.family() {
                CommitFamily::Shape => match (prim_type, float_shape.as_ref()) {
                    (Some(prim_type), Some(shape)) if prim_type.shape_editable() => {
                        field.format(shape_display_value(*field, shape, prim_type))
                    }
                    _not_editable => String::new(),
                },
                CommitFamily::Flexi => {
                    if flexi_on {
                        field.format(feature_display_value(*field, &data.extra))
                    } else {
                        String::new()
                    }
                }
                CommitFamily::Light => {
                    if light_on {
                        field.format(feature_display_value(*field, &data.extra))
                    } else {
                        String::new()
                    }
                }
                CommitFamily::Spot => {
                    if spot_on {
                        field.format(feature_display_value(*field, &data.extra))
                    } else {
                        String::new()
                    }
                }
                CommitFamily::Name | CommitFamily::Description => String::new(),
            },
        };
        if editor.value().to_string() != want {
            set_editor_text(&mut editor, &want, &mut font_cx, &mut layout_cx);
        }
    }

    // Toggle glyphs (checked state + greyed-out class).
    for (glyph, mut text, mut class_list) in &mut widgets.glyphs {
        let on = match glyph.0 {
            ParamToggle::Physical => data.is_some_and(|d| d.update_flags & FLAGS_USE_PHYSICS != 0),
            ParamToggle::Temporary => {
                data.is_some_and(|d| d.update_flags & FLAGS_TEMPORARY_ON_REZ != 0)
            }
            ParamToggle::Phantom => data.is_some_and(|d| d.update_flags & FLAGS_PHANTOM != 0),
            ParamToggle::Flexi => flexi_on,
            ParamToggle::Light => light_on,
            ParamToggle::NextModify => perm_bit(data, |p| p.next_owner, Permissions::MODIFY),
            ParamToggle::NextCopy => perm_bit(data, |p| p.next_owner, Permissions::COPY),
            ParamToggle::NextTransfer => perm_bit(data, |p| p.next_owner, Permissions::TRANSFER),
            // The share checkbox reads the group mask's modify bit, as the
            // reference's `getState` does.
            ParamToggle::ShareGroup => perm_bit(data, |p| p.group, Permissions::MODIFY),
            ParamToggle::AnyoneMove => perm_bit(data, |p| p.everyone, Permissions::MOVE),
            ParamToggle::AnyoneCopy => perm_bit(data, |p| p.everyone, Permissions::COPY),
        };
        let want = if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH };
        if text.0 != want {
            want.clone_into(&mut text.0);
        }
        set_disabled_class(&mut class_list, !enabled_for(toggle_gate(glyph.0)));
    }
    for (label, mut class_list) in &mut widgets.toggle_labels {
        set_disabled_class(&mut class_list, !enabled_for(toggle_gate(label.0)));
    }

    // Cycle labels: the value when there is one (a sculpt / mesh still shows
    // its type, greyed), a dash otherwise.
    for (value, mut text, mut class_list) in &mut widgets.cycle_values {
        let want = match value.0 {
            ParamCycle::PrimType => {
                prim_type.map(|prim_type| translator.get(prim_type.label_key()))
            }
            ParamCycle::HoleType => data.map(|data| {
                translator.get(hole_label_key(HoleType::from_byte(
                    data.shape.profile_curve,
                )))
            }),
            ParamCycle::Material => data
                .map(|data| translator.get(material_label_key(Material::from_code(data.material)))),
            ParamCycle::Group => data.map(|data| {
                if data.group.is_some() {
                    data.group_label.clone()
                } else {
                    translator.get("build-group-none")
                }
            }),
        }
        .unwrap_or_else(|| NO_VALUE.to_owned());
        if text.0 != want {
            text.0 = want;
        }
        set_disabled_class(&mut class_list, !enabled_for(cycle_gate(value.0)));
    }

    // The read-only info lines.
    for (info, mut text) in &mut widgets.infos {
        let want = match info {
            InfoText::Creator => data
                .filter(|d| !d.creator_label.is_empty())
                .map(|d| d.creator_label.clone()),
            InfoText::Owner => data
                .filter(|d| !d.owner_label.is_empty())
                .map(|d| d.owner_label.clone()),
            InfoText::YouCan => data.map(|d| {
                let mut abilities = Vec::new();
                for (bit, key) in [
                    (FLAGS_OBJECT_MODIFY, "build-perm-modify"),
                    (FLAGS_OBJECT_COPY, "build-perm-copy"),
                    (FLAGS_OBJECT_TRANSFER, "build-perm-transfer"),
                    (FLAGS_OBJECT_MOVE, "build-perm-move"),
                ] {
                    if d.update_flags & bit != 0 {
                        abilities.push(translator.get(key));
                    }
                }
                if abilities.is_empty() {
                    NO_VALUE.to_owned()
                } else {
                    abilities.join(", ")
                }
            }),
        }
        .unwrap_or_else(|| NO_VALUE.to_owned());
        if text.0 != want {
            text.0 = want;
        }
    }

    // Per-type row visibility: with nothing selected every row shows (greyed
    // by its gate, the reference's no-selection look); a selected sculpt /
    // mesh hides the shape rows (the reference's per-type visibility), and a
    // box hides the taper-profile row ("box taper does nothing").
    for (kind, mut shown) in &mut widgets.shape_rows {
        let want = match kind {
            ShapeRow::Kind => true,
            ShapeRow::Taper => {
                (!has_selection || shape_editable) && prim_type != Some(PrimTypeUi::Box)
            }
            _shape => !has_selection || shape_editable,
        };
        if shown.0 != want {
            shown.0 = want;
        }
    }
    for (variant, mut shown) in &mut widgets.swap_labels {
        let want = match variant {
            SwapLabel::ScaleTaper => !prim_type.is_some_and(PrimTypeUi::size_is_hole),
            SwapLabel::ScaleHole => prim_type.is_some_and(PrimTypeUi::size_is_hole),
            SwapLabel::AdvDimple => prim_type == Some(PrimTypeUi::Sphere),
            SwapLabel::AdvSlice => matches!(
                prim_type,
                // The default (no-selection) advanced label too — a fresh
                // selection is most often a box-family prim.
                None | Some(PrimTypeUi::Box | PrimTypeUi::Cylinder | PrimTypeUi::Prism)
            ),
            SwapLabel::AdvProfileCut => matches!(
                prim_type,
                Some(PrimTypeUi::Torus | PrimTypeUi::Tube | PrimTypeUi::Ring)
            ),
        };
        if shown.0 != want {
            shown.0 = want;
        }
    }
    // The feature sub-sections stay visible (their fields grey out via the
    // gates, the reference's cleared-and-disabled look).
    for (_rows, mut shown) in &mut widgets.feature_rows {
        if !shown.0 {
            shown.0 = true;
        }
    }

    snapshot.shown = current;
}

/// Parse a committed numeric field's value.
fn parse_numeric(kind: TextInputKind, text: &str) -> Option<f32> {
    match kind.parse(text.trim()) {
        Some(TextInputValue::Float(value)) => {
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                reason = "f64 → f32 narrowing at the field boundary; the values are bounded \
                          build-parameter entries"
            )]
            let value = value as f32;
            value.is_finite().then_some(value)
        }
        Some(TextInputValue::Integer(value)) => {
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "i64 → f32 at the field boundary; the values are small bounded entries"
            )]
            let value = value as f32;
            Some(value)
        }
        _other => None,
    }
}

/// Commit parameter-field edits on `Enter` or focus loss, dispatching by the
/// field's [`CommitFamily`] — name / description sends, or a full shape /
/// extra-params rebuild from the displayed fields.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the selection / \
              object state, the focus tracking, the field queries, and the outgoing command \
              writer"
)]
fn commit_param_fields(
    state: Res<EditToolState>,
    mut selection: ResMut<SelectionSet>,
    mut objects: ResMut<ObjectState>,
    focus: Res<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus_track: ResMut<ParamFieldFocus>,
    fields: Query<(Entity, &ParamField, &EditableText)>,
    mut snapshot: ResMut<ShownSnapshot>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !state.active {
        focus_track.last = None;
        return;
    }
    let focused_field = focus.get().filter(|entity| fields.contains(*entity));
    let enter =
        keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::NumpadEnter);
    let commit = if enter {
        focused_field
    } else if focus_track.last != focused_field {
        focus_track.last.filter(|entity| fields.contains(*entity))
    } else {
        None
    };
    focus_track.last = focused_field;
    let Some(entity) = commit else {
        return;
    };
    let Ok((_entity, field, editor)) = fields.get(entity) else {
        return;
    };
    let Some(primary_scoped) = selection.primary().map(|primary| primary.scoped) else {
        return;
    };
    let text = editor.value().to_string();

    // A helper reading any displayed field's parsed numeric value.
    let field_value = |wanted: ParamField| -> Option<f32> {
        fields.iter().find_map(|(_entity, field, editor)| {
            (*field == wanted)
                .then(|| parse_numeric(wanted.input_kind(), &editor.value().to_string()))
                .flatten()
        })
    };

    match field.family() {
        CommitFamily::Name => {
            selection.set_primary_name_description(Some(&text), None);
            commands.write(SlCommand(Command::SetObjectName {
                local_id: primary_scoped,
                name: text,
            }));
        }
        CommitFamily::Description => {
            selection.set_primary_name_description(None, Some(&text));
            commands.write(SlCommand(Command::SetObjectDescription {
                local_id: primary_scoped,
                description: text,
            }));
        }
        CommitFamily::Shape => {
            let Some(data) = owned_edit_data(&objects, &primary_scoped) else {
                return;
            };
            if data.pcode != pcode::PRIMITIVE {
                return;
            }
            let float_shape = PrimShapeFloat::from_params(&data.shape);
            let prim_type = PrimTypeUi::classify(
                &float_shape,
                data.extra.sculpt.map(|sculpt| sculpt.sculpt_type),
            );
            if !prim_type.shape_editable() {
                return;
            }
            let flexible = data.extra.flexible.is_some();
            let hole = HoleType::from_byte(data.shape.profile_curve);
            let Some(ui) = collect_shape_ui(&field_value) else {
                return;
            };
            let Some(shape) = shape_from_ui(&ui, prim_type, prim_type, hole, flexible) else {
                return;
            };
            debug!("build-params: shape commit on {primary_scoped:?}");
            commands.write(SlCommand(Command::SetObjectShape {
                local_id: primary_scoped,
                shape,
            }));
        }
        CommitFamily::Flexi => {
            let Some(data) = owned_edit_data(&objects, &primary_scoped) else {
                return;
            };
            if data.extra.flexible.is_none() {
                return;
            }
            let Some(flexi) = flexi_from_ui(&field_value) else {
                return;
            };
            let mut extra = data.extra.clone();
            extra.flexible = Some(flexi);
            objects.apply_local_extra_edit(&primary_scoped, extra.clone());
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectExtraParams {
                local_id: primary_scoped,
                params: extra,
            }));
        }
        CommitFamily::Light => {
            let Some(data) = owned_edit_data(&objects, &primary_scoped) else {
                return;
            };
            let Some(current) = data.extra.light else {
                return;
            };
            let Some(light) = light_from_ui(&field_value, current) else {
                return;
            };
            let mut extra = data.extra.clone();
            extra.light = Some(light);
            objects.apply_local_extra_edit(&primary_scoped, extra.clone());
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectExtraParams {
                local_id: primary_scoped,
                params: extra,
            }));
        }
        CommitFamily::Spot => {
            let Some(data) = owned_edit_data(&objects, &primary_scoped) else {
                return;
            };
            let Some(mut image) = data.extra.light_image.clone() else {
                return;
            };
            let (Some(fov), Some(focus_value), Some(ambiance)) = (
                field_value(ParamField::SpotFov),
                field_value(ParamField::SpotFocus),
                field_value(ParamField::SpotAmbiance),
            ) else {
                return;
            };
            image.params = Vector {
                x: fov.clamp(0.0, MAX_SPOT_FOV),
                y: focus_value.clamp(-MAX_SPOT_FOCUS, MAX_SPOT_FOCUS),
                z: ambiance.clamp(0.0, 1.0),
            };
            let mut extra = data.extra.clone();
            extra.light_image = Some(image);
            objects.apply_local_extra_edit(&primary_scoped, extra.clone());
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectExtraParams {
                local_id: primary_scoped,
                params: extra,
            }));
        }
    }
}

/// Parse every displayed shape field into a [`ShapeUiValues`], or `None` when
/// any field holds an unparsable entry (the commit is then abandoned, as the
/// transform rows do).
fn collect_shape_ui(values: &dyn Fn(ParamField) -> Option<f32>) -> Option<ShapeUiValues> {
    Some(ShapeUiValues {
        cut_begin: values(ParamField::CutBegin)?,
        cut_end: values(ParamField::CutEnd)?,
        hollow_percent: values(ParamField::Hollow)?,
        twist_begin_deg: values(ParamField::TwistBegin)?,
        twist_end_deg: values(ParamField::TwistEnd)?,
        scale_x: values(ParamField::ScaleX)?,
        scale_y: values(ParamField::ScaleY)?,
        shear_x: values(ParamField::ShearX)?,
        shear_y: values(ParamField::ShearY)?,
        adv_begin: values(ParamField::AdvBegin)?,
        adv_end: values(ParamField::AdvEnd)?,
        taper_x: values(ParamField::TaperX)?,
        taper_y: values(ParamField::TaperY)?,
        radius_offset: values(ParamField::RadiusOffset)?,
        revolutions: values(ParamField::Revolutions)?,
        skew: values(ParamField::Skew)?,
    })
}

// ---------------------------------------------------------------------------
// The toggle / cycle observers.
// ---------------------------------------------------------------------------

/// The observer every [`ParamToggle`] row runs on press: flip the toggle's
/// wire state for the primary selection and send the corresponding message
/// (the toggle is read off the pressed row's component).
fn handle_toggle_press(
    press: On<Pointer<Press>>,
    toggles: Query<&ParamToggle>,
    mut selection: ResMut<SelectionSet>,
    mut objects: ResMut<ObjectState>,
    mut snapshot: ResMut<ShownSnapshot>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(&toggle) = toggles.get(press.entity) else {
        return;
    };
    let Some(primary_scoped) = selection.primary().map(|primary| primary.scoped) else {
        return;
    };
    let Some(data) = owned_edit_data(&objects, &primary_scoped) else {
        return;
    };
    match toggle {
        ParamToggle::Physical | ParamToggle::Temporary | ParamToggle::Phantom => {
            let bit = match toggle {
                ParamToggle::Physical => FLAGS_USE_PHYSICS,
                ParamToggle::Temporary => FLAGS_TEMPORARY_ON_REZ,
                _phantom => FLAGS_PHANTOM,
            };
            let flags = data.update_flags ^ bit;
            let settings = ObjectFlagSettings {
                use_physics: flags & FLAGS_USE_PHYSICS != 0,
                is_temporary: flags & FLAGS_TEMPORARY_ON_REZ != 0,
                is_phantom: flags & FLAGS_PHANTOM != 0,
                casts_shadows: flags & FLAGS_CAST_SHADOWS != 0,
            };
            objects.apply_local_flag_edit(&primary_scoped, bit, flags & bit != 0);
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectFlags {
                local_id: primary_scoped,
                flags: settings,
            }));
        }
        ParamToggle::NextModify
        | ParamToggle::NextCopy
        | ParamToggle::NextTransfer
        | ParamToggle::ShareGroup
        | ParamToggle::AnyoneMove
        | ParamToggle::AnyoneCopy => {
            let (field, mask) = match toggle {
                ParamToggle::NextModify => (PermissionField::NextOwner, Permissions::MODIFY),
                ParamToggle::NextCopy => (PermissionField::NextOwner, Permissions::COPY),
                ParamToggle::NextTransfer => (PermissionField::NextOwner, Permissions::TRANSFER),
                ParamToggle::ShareGroup => (PermissionField::Group, GROUP_SHARE_MASK),
                ParamToggle::AnyoneMove => (PermissionField::Everyone, Permissions::MOVE),
                _anyone_copy => (PermissionField::Everyone, Permissions::COPY),
            };
            let Some(properties) = selection.primary_properties_mut() else {
                // The masks are unknown until the properties reply lands;
                // there is nothing sound to toggle against yet.
                return;
            };
            let mask_ref = match field {
                PermissionField::NextOwner => &mut properties.permissions.next_owner,
                PermissionField::Group => &mut properties.permissions.group,
                _everyone => &mut properties.permissions.everyone,
            };
            // The checkbox state reads the mask's first bit (the reference's
            // share checkbox keys off modify); toggling sets or clears the
            // whole mask.
            let set = !mask_ref.contains(match toggle {
                ParamToggle::ShareGroup => Permissions::MODIFY,
                _single_bit => mask,
            });
            *mask_ref = if set {
                mask_ref.union(mask)
            } else {
                mask_ref.difference(mask)
            };
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectPermissions {
                local_ids: vec![primary_scoped],
                field,
                set,
                mask,
            }));
            // Re-request the properties so the simulator's clamped masks
            // (e.g. a no-transfer object refusing copy changes) replace the
            // local echo.
            commands.write(SlCommand(Command::RequestObjectProperties {
                local_ids: vec![primary_scoped],
            }));
        }
        ParamToggle::Flexi => {
            // Only a plain linear-path prim can be flexible (the reference's
            // `canBeFlexible`).
            if data.pcode != pcode::PRIMITIVE || data.extra.sculpt.is_some() {
                return;
            }
            let shape = PrimShapeFloat::from_params(&data.shape);
            let enabling = data.extra.flexible.is_none();
            if enabling && !matches!(shape.path_curve, PathCurve::Line | PathCurve::Flexible) {
                return;
            }
            let mut extra = data.extra.clone();
            let mut new_shape = data.shape;
            if enabling {
                extra.flexible = Some(FLEXI_DEFAULTS);
                new_shape.path_curve = PathCurve::Flexible.to_byte();
            } else {
                extra.flexible = None;
                if shape.path_curve == PathCurve::Flexible {
                    new_shape.path_curve = PathCurve::Line.to_byte();
                }
            }
            objects.apply_local_extra_edit(&primary_scoped, extra.clone());
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectExtraParams {
                local_id: primary_scoped,
                params: extra,
            }));
            // The reference couples the flexi toggle with a shape update
            // (LINE <-> FLEXIBLE path curve).
            if new_shape.path_curve != data.shape.path_curve {
                commands.write(SlCommand(Command::SetObjectShape {
                    local_id: primary_scoped,
                    shape: new_shape,
                }));
            }
        }
        ParamToggle::Light => {
            // Only a volume prim can be a light (the reference's gate);
            // sculpts qualify.
            if data.pcode != pcode::PRIMITIVE {
                return;
            }
            let mut extra = data.extra.clone();
            if extra.light.take().is_none() {
                extra.light = Some(LIGHT_DEFAULTS);
            }
            objects.apply_local_extra_edit(&primary_scoped, extra.clone());
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectExtraParams {
                local_id: primary_scoped,
                params: extra,
            }));
        }
    }
}

/// The observer every [`ParamCycle`] button runs on press: advance to the
/// next entry and send the corresponding message (the cycle is read off the
/// pressed button's component).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources / queries: the pressed \
              button's cycle, the selection / object / group state, the snapshot, the shape \
              fields, and the outgoing command writer"
)]
fn handle_cycle_press(
    press: On<Pointer<Press>>,
    cycles: Query<&ParamCycle>,
    mut selection: ResMut<SelectionSet>,
    mut objects: ResMut<ObjectState>,
    groups: Res<GroupsModel>,
    mut snapshot: ResMut<ShownSnapshot>,
    fields: Query<(&ParamField, &EditableText)>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(&cycle) = cycles.get(press.entity) else {
        return;
    };
    let Some(primary_scoped) = selection.primary().map(|primary| primary.scoped) else {
        return;
    };
    let Some(data) = owned_edit_data(&objects, &primary_scoped) else {
        return;
    };
    match cycle {
        ParamCycle::Material => {
            let next = next_material(Material::from_code(data.material));
            objects.apply_local_material_edit(&primary_scoped, next.to_code());
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectMaterial {
                local_id: primary_scoped,
                material: next,
            }));
        }
        ParamCycle::Group => {
            // Cycle none -> each of the agent's groups -> none (the
            // reference opens a group picker; the cycle is the combo
            // stand-in until [[viewer-ui-combo-widget]]).
            let ids = groups.group_ids();
            let Some(properties) = selection.primary_properties_mut() else {
                return;
            };
            let next = match properties.group {
                None => ids.first().copied(),
                Some(current) => ids
                    .iter()
                    .position(|id| *id == current)
                    .and_then(|index| ids.get(index.saturating_add(1)))
                    .copied(),
            };
            properties.group = next;
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectGroup {
                local_ids: vec![primary_scoped],
                group_id: next.unwrap_or_else(|| GroupKey::from(Uuid::nil())),
            }));
            commands.write(SlCommand(Command::RequestObjectProperties {
                local_ids: vec![primary_scoped],
            }));
        }
        ParamCycle::PrimType | ParamCycle::HoleType => {
            if data.pcode != pcode::PRIMITIVE {
                return;
            }
            let float_shape = PrimShapeFloat::from_params(&data.shape);
            let current_type = PrimTypeUi::classify(
                &float_shape,
                data.extra.sculpt.map(|sculpt| sculpt.sculpt_type),
            );
            if !current_type.shape_editable() {
                return;
            }
            let (new_type, hole) = match cycle {
                ParamCycle::PrimType => (
                    current_type.next(),
                    HoleType::from_byte(data.shape.profile_curve),
                ),
                _hole => (
                    current_type,
                    next_hole_type(HoleType::from_byte(data.shape.profile_curve)),
                ),
            };
            let flexible = data.extra.flexible.is_some();
            let field_value = |wanted: ParamField| -> Option<f32> {
                fields.iter().find_map(|(field, editor)| {
                    (*field == wanted)
                        .then(|| parse_numeric(wanted.input_kind(), &editor.value().to_string()))
                        .flatten()
                })
            };
            // The displayed values are reinterpreted under the NEW type (the
            // reference's `getVolumeParams` semantics), with the taper
            // inversion keyed off the OLD (displayed) type.
            let ui = collect_shape_ui(&field_value)
                .unwrap_or_else(|| shape_ui_from_data(&float_shape, current_type));
            let Some(shape) = shape_from_ui(&ui, new_type, current_type, hole, flexible) else {
                return;
            };
            snapshot.shown = None;
            commands.write(SlCommand(Command::SetObjectShape {
                local_id: primary_scoped,
                shape,
            }));
        }
    }
}

/// The observer every [`ParamAction`] button runs on press.
fn handle_action_press(
    press: On<Pointer<Press>>,
    actions: Query<&ParamAction>,
    selection: Res<SelectionSet>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(&action) = actions.get(press.entity) else {
        return;
    };
    match action {
        ParamAction::Deed => {
            let Some(primary) = selection.primary() else {
                return;
            };
            // Deeding needs the object's group (it becomes the owner); the
            // gate greys the button until one is set.
            let Some(group) = primary
                .properties
                .as_ref()
                .and_then(|properties| properties.group)
            else {
                return;
            };
            commands.write(SlCommand(Command::DeedObjectsToGroup {
                local_ids: vec![primary.scoped],
                group_id: group,
            }));
            commands.write(SlCommand(Command::RequestObjectProperties {
                local_ids: vec![primary.scoped],
            }));
        }
    }
}

/// The [`ShapeUiValues`] the current shape *would* display under `prim_type`
/// — the fallback when a cycle commit finds an unparsable field.
fn shape_ui_from_data(shape: &PrimShapeFloat, prim_type: PrimTypeUi) -> ShapeUiValues {
    ShapeUiValues {
        cut_begin: shape_display_value(ParamField::CutBegin, shape, prim_type),
        cut_end: shape_display_value(ParamField::CutEnd, shape, prim_type),
        hollow_percent: shape_display_value(ParamField::Hollow, shape, prim_type),
        twist_begin_deg: shape_display_value(ParamField::TwistBegin, shape, prim_type),
        twist_end_deg: shape_display_value(ParamField::TwistEnd, shape, prim_type),
        scale_x: shape_display_value(ParamField::ScaleX, shape, prim_type),
        scale_y: shape_display_value(ParamField::ScaleY, shape, prim_type),
        shear_x: shape_display_value(ParamField::ShearX, shape, prim_type),
        shear_y: shape_display_value(ParamField::ShearY, shape, prim_type),
        adv_begin: shape_display_value(ParamField::AdvBegin, shape, prim_type),
        adv_end: shape_display_value(ParamField::AdvEnd, shape, prim_type),
        taper_x: shape_display_value(ParamField::TaperX, shape, prim_type),
        taper_y: shape_display_value(ParamField::TaperY, shape, prim_type),
        radius_offset: shape_display_value(ParamField::RadiusOffset, shape, prim_type),
        revolutions: shape_display_value(ParamField::Revolutions, shape, prim_type),
        skew: shape_display_value(ParamField::Skew, shape, prim_type),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FLEXI_DEFAULTS, LIGHT_DEFAULTS, ParamField, PrimTypeUi, ShapeUiValues, clamp_cut_pair,
        linear_byte_to_srgb, next_hole_type, next_material, quantize_cut_begin, quantize_cut_end,
        quantize_hollow, quantize_path_scale, quantize_revolutions, quantize_shear,
        quantize_signed, shape_display_value, shape_from_ui, srgb_to_linear_byte,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{HoleType, Material, PrimShapeFloat, PrimShapeParams};

    /// A default (unit-box-like) quantized shape.
    fn box_params() -> PrimShapeParams {
        PrimShapeParams {
            // LL_PCODE_PATH_LINE / LL_PCODE_PROFILE_SQUARE.
            path_curve: 0x10,
            profile_curve: 0x01,
            path_begin: 0,
            path_end: 0,
            path_scale_x: 100,
            path_scale_y: 100,
            ..PrimShapeParams::default()
        }
    }

    /// The display values a default box shows: full cut, no hollow, no twist,
    /// zero displayed taper (`1 − ratio` with ratio 1).
    #[test]
    fn box_display_values() {
        let shape = PrimShapeFloat::from_params(&box_params());
        let prim_type = PrimTypeUi::classify(&shape, None);
        assert_eq!(prim_type, PrimTypeUi::Box);
        let value = |field| shape_display_value(field, &shape, prim_type);
        assert!((value(ParamField::CutBegin)).abs() < 1.0e-6);
        assert!((value(ParamField::CutEnd) - 1.0).abs() < 1.0e-6);
        assert!((value(ParamField::Hollow)).abs() < 1.0e-6);
        assert!((value(ParamField::ScaleX)).abs() < 1.0e-6);
        assert!((value(ParamField::ScaleY)).abs() < 1.0e-6);
    }

    /// Quantize ∘ dequantize is the identity on the wire grid: a shape built
    /// from its own display values round-trips exactly.
    #[test]
    fn shape_round_trip() {
        let mut params = box_params();
        params.path_begin = 5000;
        params.path_end = 5000;
        params.profile_begin = 2500;
        params.profile_end = 10000;
        params.profile_hollow = 20000;
        params.path_twist = 25;
        params.path_twist_begin = -50;
        params.path_scale_x = 150;
        params.path_scale_y = 50;
        params.path_shear_x = 30;
        params.path_shear_y = (-20_i8).cast_unsigned();
        let shape = PrimShapeFloat::from_params(&params);
        let prim_type = PrimTypeUi::classify(&shape, None);
        let ui = super::shape_ui_from_data(&shape, prim_type);
        let Some(rebuilt) = shape_from_ui(&ui, prim_type, prim_type, HoleType::Same, false) else {
            unreachable!("a box is shape-editable")
        };
        assert_eq!(rebuilt, params);
    }

    /// A torus round-trips through the S/T-flipped display too.
    #[test]
    fn torus_round_trip() {
        let params = PrimShapeParams {
            // LL_PCODE_PATH_CIRCLE / LL_PCODE_PROFILE_CIRCLE.
            path_curve: 0x20,
            profile_curve: 0x00,
            path_begin: 2500,
            path_end: 5000,
            profile_begin: 5000,
            profile_end: 2500,
            profile_hollow: 10000,
            // Hole size 0.25 / 0.25.
            path_scale_x: 175,
            path_scale_y: 175,
            path_twist: 50,
            path_twist_begin: -25,
            path_radius_offset: 30,
            path_taper_x: -40,
            path_taper_y: 40,
            path_revolutions: 100,
            path_skew: 20,
            ..PrimShapeParams::default()
        };
        let shape = PrimShapeFloat::from_params(&params);
        let prim_type = PrimTypeUi::classify(&shape, None);
        assert_eq!(prim_type, PrimTypeUi::Torus);
        let ui = super::shape_ui_from_data(&shape, prim_type);
        // The torus "Path Cut" row must carry the *path* begin/end.
        assert!((ui.cut_begin - shape.path_begin).abs() < 1.0e-6);
        assert!((ui.adv_begin - shape.profile_begin).abs() < 1.0e-6);
        let Some(rebuilt) = shape_from_ui(&ui, prim_type, prim_type, HoleType::Same, false) else {
            unreachable!("a torus is shape-editable")
        };
        assert_eq!(rebuilt, params);
    }

    /// The classification matches the reference's `getState` table, including
    /// the sphere-vs-torus `path_scale_y > 0.75` split and the sculpt / mesh
    /// override.
    #[test]
    fn classification_matches_reference() {
        let classify = |path: u8, profile: u8, scale_y: u8| {
            let params = PrimShapeParams {
                path_curve: path,
                profile_curve: profile,
                path_scale_y: scale_y,
                ..PrimShapeParams::default()
            };
            PrimTypeUi::classify(&PrimShapeFloat::from_params(&params), None)
        };
        assert_eq!(classify(0x10, 0x01, 100), PrimTypeUi::Box);
        assert_eq!(classify(0x10, 0x00, 100), PrimTypeUi::Cylinder);
        assert_eq!(classify(0x10, 0x03, 100), PrimTypeUi::Prism);
        // Flexible path counts as linear.
        assert_eq!(classify(0x80, 0x00, 100), PrimTypeUi::Cylinder);
        assert_eq!(classify(0x20, 0x05, 100), PrimTypeUi::Sphere);
        // Circle path + circle profile splits on the dequantized scale_y:
        // raw 100 → 1.0 (> 0.75, a squashed sphere), raw 175 → 0.25 (torus).
        assert_eq!(classify(0x20, 0x00, 100), PrimTypeUi::Sphere);
        assert_eq!(classify(0x20, 0x00, 175), PrimTypeUi::Torus);
        assert_eq!(classify(0x20, 0x01, 175), PrimTypeUi::Tube);
        assert_eq!(classify(0x20, 0x03, 175), PrimTypeUi::Ring);
        assert_eq!(classify(0x30, 0x00, 100), PrimTypeUi::Sphere);
        // A sculpt / mesh block overrides the curve classification.
        let box_shape = PrimShapeFloat::from_params(&box_params());
        assert_eq!(
            PrimTypeUi::classify(&box_shape, Some(1)),
            PrimTypeUi::Sculpt
        );
        assert_eq!(PrimTypeUi::classify(&box_shape, Some(5)), PrimTypeUi::Mesh);
    }

    /// Switching type applies the reference's per-type curve bytes, and the
    /// box-family taper inversion keys off the previously displayed type.
    #[test]
    fn type_switch_defaults() {
        // A box displaying taper 0.25 (ratio 0.75) switched to a torus: the
        // ratio must be un-inverted with the *box* convention, then clamped
        // into the torus hole-size range.
        let ui = ShapeUiValues {
            cut_begin: 0.0,
            cut_end: 1.0,
            hollow_percent: 0.0,
            twist_begin_deg: 0.0,
            twist_end_deg: 0.0,
            scale_x: 0.25,
            scale_y: 0.8,
            shear_x: 0.0,
            shear_y: 0.0,
            adv_begin: 0.0,
            adv_end: 1.0,
            taper_x: 0.0,
            taper_y: 0.0,
            radius_offset: 0.0,
            revolutions: 1.0,
            skew: 0.0,
        };
        let Some(torus) = shape_from_ui(
            &ui,
            PrimTypeUi::Torus,
            PrimTypeUi::Box,
            HoleType::Same,
            false,
        ) else {
            unreachable!("a torus is buildable")
        };
        assert_eq!(torus.path_curve, 0x20);
        assert_eq!(torus.profile_curve, 0x00);
        // 1 − 0.25 = 0.75 hole X; 1 − 0.8 = 0.2 hole Y (both in range).
        assert_eq!(torus.path_scale_x, 125);
        assert_eq!(torus.path_scale_y, 180);
        // The sculpt / mesh types build nothing.
        assert!(
            shape_from_ui(
                &ui,
                PrimTypeUi::Mesh,
                PrimTypeUi::Box,
                HoleType::Same,
                false
            )
            .is_none()
        );
    }

    /// The hollow-shape nibble rides the profile byte.
    #[test]
    fn hole_type_nibble() {
        let ui =
            super::shape_ui_from_data(&PrimShapeFloat::from_params(&box_params()), PrimTypeUi::Box);
        let Some(shape) = shape_from_ui(
            &ui,
            PrimTypeUi::Box,
            PrimTypeUi::Box,
            HoleType::Square,
            false,
        ) else {
            unreachable!("a box is buildable")
        };
        assert_eq!(shape.profile_curve, 0x21);
        assert_eq!(next_hole_type(HoleType::Same), HoleType::Circle);
        assert_eq!(next_hole_type(HoleType::Triangle), HoleType::Same);
    }

    /// The quantizers are the exact inverses of `sl-prim`'s dequantization
    /// formulas.
    #[test]
    fn quantizer_inverses() {
        assert_eq!(quantize_cut_begin(0.1), 5000);
        assert_eq!(quantize_cut_end(0.9), 5000);
        assert_eq!(quantize_hollow(0.4), 20000);
        // Ratio 1.0 → raw 100; ratio 0.25 → raw 175.
        assert_eq!(quantize_path_scale(1.0), 100);
        assert_eq!(quantize_path_scale(0.25), 175);
        assert_eq!(quantize_signed(-0.5), -50);
        assert_eq!(quantize_shear(-0.2), (-20_i8).cast_unsigned());
        // Revolutions 1.0 → 0; 2.5 → 100.
        assert_eq!(quantize_revolutions(1.0), 0);
        assert_eq!(quantize_revolutions(2.5), 100);
    }

    /// The cut clamp keeps the reference's minimum 0.02 slice.
    #[test]
    fn cut_gap_clamped() {
        let (begin, end) = clamp_cut_pair(0.99, 1.0);
        assert!((begin - 0.98).abs() < 1.0e-6);
        assert!((end - 1.0).abs() < 1.0e-6);
        let (begin, end) = clamp_cut_pair(0.5, 0.3);
        assert!((begin - 0.28).abs() < 1.0e-5);
        assert!((end - 0.3).abs() < 1.0e-6);
    }

    /// The one-decimal sRGB display round-trips every linear wire byte: an
    /// unedited colour channel commits back to the byte it displayed.
    #[test]
    fn srgb_round_trip() {
        for byte in 0_u16..=255 {
            let Ok(byte) = u8::try_from(byte) else {
                unreachable!("the loop range is bounded to bytes")
            };
            let displayed = ParamField::LightRed.format(linear_byte_to_srgb(byte));
            let Ok(parsed) = displayed.parse::<f32>() else {
                unreachable!("the display is a plain decimal")
            };
            assert_eq!(srgb_to_linear_byte(parsed), byte, "display {displayed}");
        }
    }

    /// The material cycle covers the reference combo (and skips the legacy
    /// light material).
    #[test]
    fn material_cycle() {
        let mut material = Material::Stone;
        let mut seen = vec![material];
        for _step in 0..6 {
            material = next_material(material);
            seen.push(material);
        }
        assert_eq!(next_material(material), Material::Stone);
        assert!(!seen.contains(&Material::Light));
        assert_eq!(next_material(Material::Light), Material::Stone);
    }

    /// The enable defaults match the reference's seeds.
    #[test]
    fn feature_defaults() {
        assert_eq!(FLEXI_DEFAULTS.softness, 2);
        assert!((FLEXI_DEFAULTS.gravity - 0.3).abs() < 1.0e-6);
        assert!((FLEXI_DEFAULTS.air_friction - 2.0).abs() < 1.0e-6);
        assert!((FLEXI_DEFAULTS.tension - 1.0).abs() < 1.0e-6);
        assert_eq!(LIGHT_DEFAULTS.color, [255, 255, 255, 255]);
        assert!((LIGHT_DEFAULTS.radius - 10.0).abs() < 1.0e-6);
        assert!((LIGHT_DEFAULTS.falloff - 0.75).abs() < 1.0e-6);
    }
}
