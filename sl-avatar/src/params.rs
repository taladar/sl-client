//! Parsing of the standard Linden `character/` visual-param table
//! (`avatar_lad.xml`'s `<param>` elements) and the mapping from a wire
//! `AvatarAppearance.visual_params` byte vector onto typed param values (P12.4).
//!
//! Second Life describes an avatar's shape and appearance as a set of *visual
//! parameters*, each a scalar `weight` between a per-param `value_min` and
//! `value_max`. A param's effect is one of:
//!
//! - **morph** ([`ParamEffect::Morph`], `<param_morph>`) — blends a named
//!   morph-target delta in the base-body meshes; the target is resolved by the
//!   param's [`name`](VisualParam::name) against each base part's morph table
//!   (the morph data itself lives in the `.llm`, decoded by
//!   [`crate::basemesh`]).
//! - **skeletal** ([`ParamEffect::Skeleton`], `<param_skeleton>`) — scales and
//!   optionally offsets skeleton bones (proportions: height, limb/head scale).
//! - **driver** ([`ParamEffect::Driver`], `<param_driver>`) — drives a list of
//!   other params over sub-ranges of its own weight (a single slider that moves
//!   several morphs together).
//! - **colour** ([`ParamEffect::Color`], `<param_color>`) and **alpha**
//!   ([`ParamEffect::Alpha`], `<param_alpha>`) — texture-layer compositing
//!   inputs used by the bake, kept here because they still occupy wire slots.
//!
//! Only params in the transmitted groups ([`ParamGroup::is_transmitted`]) appear
//! in `AvatarAppearance.visual_params`. The viewer sends them **sorted by id
//! ascending** (Firestorm iterates its `std::map<S32, LLVisualParam*>` in key
//! order and packs the tweakable / transmit-not-tweakable groups), so byte `i`
//! of the vector is the `i`-th [`VisualParams::transmitted`] param. Each byte is
//! dequantized with the same `U8_to_F32` ramp the reference viewer uses,
//! including its snap-to-zero step.
//!
//! Like the rest of the crate this module is I/O-free: it parses from a borrowed
//! `&str` and stays in Second Life's own units (weights are raw param values;
//! the Bevy morph/skeleton application happens in later phases).

use std::collections::HashMap;

/// An error returned while parsing the `avatar_lad.xml` visual-param table.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ParamError {
    /// The XML itself was malformed and could not be parsed.
    #[error("malformed avatar XML")]
    Xml(#[from] roxmltree::Error),
    /// The document's root element was not `<linden_avatar>`.
    #[error("expected root element `linden_avatar`, found `{found}`")]
    UnexpectedRoot {
        /// The element name actually found.
        found: String,
    },
    /// A `<param>` (or a child element) lacked a required attribute.
    #[error("`{element}` element is missing required attribute `{attribute}`")]
    MissingAttribute {
        /// The element that lacked the attribute.
        element: &'static str,
        /// The name of the missing attribute.
        attribute: &'static str,
    },
    /// A numeric attribute could not be parsed.
    #[error("attribute `{attribute}` is not a valid number: {value:?}")]
    BadNumber {
        /// The offending attribute name.
        attribute: &'static str,
        /// The raw attribute value.
        value: String,
    },
    /// An attribute expected to hold three space-separated floats could not be
    /// parsed as such.
    #[error("attribute `{attribute}` is not a 3-vector: {value:?}")]
    BadVector {
        /// The offending attribute name.
        attribute: &'static str,
        /// The raw attribute value.
        value: String,
    },
    /// A `param_color` `<value>` `color` attribute was not four `0..=255`
    /// components.
    #[error("`color` attribute is not an RGBA quad: {value:?}")]
    BadColor {
        /// The raw attribute value.
        value: String,
    },
}

/// The Linden visual-param group (`group` attribute), matching Firestorm's
/// `EVisualParamGroup`. The group decides whether a param is user-tweakable and
/// whether it is transmitted in the appearance message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamGroup {
    /// `0` — user-tweakable in the appearance editor **and** transmitted.
    Tweakable,
    /// `1` — animatable (driven by animation), not transmitted.
    Animatable,
    /// `2` — tweakable but not transmitted.
    TweakableNoTransmit,
    /// `3` — deprecated params that used to be tweakable; still transmitted.
    TransmitNotTweakable,
}

impl ParamGroup {
    /// Map the numeric `group` attribute (`0`..=`3`) onto the enum; an
    /// out-of-range or absent value defaults to [`Tweakable`](Self::Tweakable),
    /// as the reference viewer does.
    #[must_use]
    pub const fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Animatable,
            2 => Self::TweakableNoTransmit,
            3 => Self::TransmitNotTweakable,
            _ => Self::Tweakable,
        }
    }

    /// Whether params in this group are packed into
    /// `AvatarAppearance.visual_params` — the tweakable and
    /// transmit-not-tweakable groups (Firestorm `sendAgentSetAppearance`).
    #[must_use]
    pub const fn is_transmitted(self) -> bool {
        matches!(self, Self::Tweakable | Self::TransmitNotTweakable)
    }
}

/// Which sex a param applies to (the optional `sex` attribute).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamSex {
    /// Applies to both sexes (the default when `sex` is absent).
    Both,
    /// Applies only when the avatar is male.
    Male,
    /// Applies only when the avatar is female.
    Female,
}

/// A single bone deformation within a [`ParamEffect::Skeleton`] param: a scale
/// (and optional translation offset) applied to one skeleton bone.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneOffset {
    /// The skeleton bone this deformation targets (e.g. `mNeck`), matched to a
    /// [`crate::skeleton::Joint`] by name/alias.
    pub bone: String,
    /// Per-axis scale multiplier applied at full param weight (Z-up).
    pub scale: [f32; 3],
    /// Optional per-axis translation offset applied at full weight, in metres;
    /// `None` when the bone has no `offset` attribute (Firestorm's `haspos`).
    pub offset: Option<[f32; 3]>,
}

/// A driven entry within a [`ParamEffect::Driver`] param: which param is driven
/// and over what sub-range of the driver's weight.
///
/// The four thresholds form the trapezoid ramp described in Firestorm's
/// `LLDriverParamInfo::parseXml`: the driven param ramps up between `min1` and
/// `max1`, holds at full between `max1` and `max2`, then ramps down between
/// `max2` and `min2`. Absent thresholds default to the driver's own weight
/// bounds (`min1 = value_min`, `max1 = max2 = min2 = value_max`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrivenParam {
    /// The id of the driven param.
    pub id: i32,
    /// Driver weight at which the driven param starts ramping up.
    pub min1: f32,
    /// Driver weight at which the driven param reaches full.
    pub max1: f32,
    /// Driver weight at which the driven param starts ramping down from full.
    pub max2: f32,
    /// Driver weight at which the driven param returns to zero.
    pub min2: f32,
}

/// The typed effect a [`VisualParam`] has on the avatar, one per recognized
/// child element of the `<param>`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ParamEffect {
    /// `<param_morph>` — blends a base-mesh morph target resolved by the param's
    /// [`name`](VisualParam::name).
    Morph,
    /// `<param_skeleton>` — scales/offsets the listed skeleton bones.
    Skeleton(Vec<BoneOffset>),
    /// `<param_driver>` — drives the listed params.
    Driver(Vec<DrivenParam>),
    /// `<param_color>` — a texture-layer colour ramp (RGBA stops) plus the
    /// operation combining it with the layers below; a baking input (see
    /// [`ColorRamp`]), kept so the param still occupies its wire slot.
    Color(ColorRamp),
    /// `<param_alpha>` — a texture-layer alpha mask; a baking input.
    Alpha,
    /// No recognized effect child element.
    None,
}

/// How a [`ParamEffect::Color`] param's evaluated colour combines with the
/// running net colour of the layers below it, mirroring the reference viewer's
/// `LLTexLayerParamColor::EColorOperation` (`<param_color operation="…">`). The
/// reference viewer defaults an unrecognized / absent operation (including the
/// XML's `"add_multiply"`) to [`Add`](ColorOp::Add).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorOp {
    /// `OP_ADD`: add this param's colour to the running total (also the default).
    #[default]
    Add,
    /// `OP_MULTIPLY`: multiply the running total by this param's colour.
    Multiply,
    /// `OP_BLEND`: linearly interpolate the running total towards this param's
    /// colour by the param's weight (must have exactly one stop).
    Blend,
}

impl ColorOp {
    /// Classify a `<param_color operation="…">` string; anything the reference
    /// viewer does not recognize (including `"add_multiply"`) is [`Self::Add`].
    #[must_use]
    pub fn from_operation(operation: &str) -> Self {
        match operation {
            "multiply" => Self::Multiply,
            "blend" => Self::Blend,
            _ => Self::Add,
        }
    }
}

/// A texture-layer colour ramp from a `<param_color>`: the ordered RGBA stops the
/// param interpolates between as its weight sweeps `0..=1`, plus the [`ColorOp`]
/// combining the result with the layers below. This is the reference viewer's
/// `LLTexLayerParamColorInfo`; [`ColorRamp::net_color`] is its `getNetColor`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorRamp {
    /// How the evaluated colour combines with the running net colour.
    pub operation: ColorOp,
    /// The ordered RGBA stops (`0..=255`), at least one; `getNetColor`
    /// interpolates across them by the param's weight.
    pub stops: Vec<[u8; 4]>,
}

impl ColorRamp {
    /// The ramp colour at `weight` as linear RGBA in `0.0..=1.0`, replicating the
    /// reference viewer's `LLTexLayerParamColor::getNetColor`: `weight` (clamped
    /// to `0..=1`) is scaled across `stops.len() - 1` and the two bracketing
    /// stops are linearly interpolated. An empty ramp is opaque white.
    #[must_use]
    pub fn net_color(&self, weight: f32) -> [f32; 4] {
        let last = self.stops.len().saturating_sub(1);
        let Some(&first) = self.stops.first() else {
            return [1.0, 1.0, 1.0, 1.0];
        };
        let weight = weight.clamp(0.0, 1.0);
        // `weight * last`, split into the lower stop index and the fraction into
        // the next stop.
        let scaled = weight * f32_from_usize(last);
        let index_start = usize_floor(scaled).min(last);
        if index_start == last {
            return rgba_to_unit(self.stops.get(last).copied().unwrap_or(first));
        }
        let frac = scaled - f32_from_usize(index_start);
        let start = rgba_to_unit(self.stops.get(index_start).copied().unwrap_or(first));
        let end = rgba_to_unit(
            self.stops
                .get(index_start.saturating_add(1))
                .copied()
                .unwrap_or(first),
        );
        [
            start[0] + (end[0] - start[0]) * frac,
            start[1] + (end[1] - start[1]) * frac,
            start[2] + (end[2] - start[2]) * frac,
            start[3] + (end[3] - start[3]) * frac,
        ]
    }
}

/// One visual parameter parsed from an `avatar_lad.xml` `<param>` element.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualParam {
    /// The param's numeric id (the wire key; params are transmitted in ascending
    /// id order).
    pub id: i32,
    /// The param's group, deciding tweakability and transmission.
    pub group: ParamGroup,
    /// The param's internal name (original case, e.g. `Big_Brow`); a
    /// [`ParamEffect::Morph`] resolves its morph target by this name.
    pub name: String,
    /// The human-readable label shown in the editor, if the `label` attribute is
    /// present (else falls back to the name at display time).
    pub label: Option<String>,
    /// The wearable this param belongs to (`shape`, `skin`, `hair`, …), if the
    /// `wearable` attribute is present.
    pub wearable: Option<String>,
    /// Which sex the param applies to.
    pub sex: ParamSex,
    /// The minimum weight (`value_min`), reached at wire byte `0`.
    pub min: f32,
    /// The maximum weight (`value_max`), reached at wire byte `255`.
    pub max: f32,
    /// The default weight (`value_default`, clamped to `[min, max]`; `0` when
    /// absent), used where the appearance vector does not carry the param.
    pub default: f32,
    /// The param's typed effect.
    pub effect: ParamEffect,
}

impl VisualParam {
    /// Dequantize a wire byte into this param's weight, replicating the
    /// reference viewer's `U8_to_F32` ramp — a linear map from `[0, 255]` onto
    /// `[min, max]` with a snap-to-zero within one quantization step.
    #[must_use]
    pub fn weight_from_byte(&self, byte: u8) -> f32 {
        u8_to_f32(byte, self.min, self.max)
    }

    /// Quantize this param's weight into its wire byte, the inverse of
    /// [`weight_from_byte`](Self::weight_from_byte) (Firestorm's `F32_to_U8`).
    #[must_use]
    pub fn byte_from_weight(&self, weight: f32) -> u8 {
        f32_to_u8(weight, self.min, self.max)
    }

    /// Whether this param is packed into `AvatarAppearance.visual_params`.
    #[must_use]
    pub const fn is_transmitted(&self) -> bool {
        self.group.is_transmitted()
    }
}

/// The parsed visual-param table from `avatar_lad.xml`: every `<param>` keyed by
/// id, plus the transmitted subset in wire order.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `VisualParams` reads clearly"
)]
#[derive(Clone, Debug)]
pub struct VisualParams {
    /// All params, sorted by ascending id (deterministic; also the wire order of
    /// the transmitted subset once filtered).
    params: Vec<VisualParam>,
    /// Indices into [`Self::params`] for the transmitted params, in ascending id
    /// (wire) order.
    transmitted: Vec<usize>,
    /// Lookup from a param id to its index in [`Self::params`].
    by_id: HashMap<i32, usize>,
}

impl VisualParams {
    /// Parse an `avatar_lad.xml` document's visual-param table from its text.
    ///
    /// Collects every `<param>` element anywhere in the document (skeleton,
    /// mesh, layer-set, and driver sections), deduplicating by id (a later
    /// definition wins, mirroring the reference viewer's `addVisualParam` map
    /// overwrite), and sorts by ascending id.
    ///
    /// # Errors
    ///
    /// Returns [`ParamError`] if the XML is malformed, the root is not
    /// `<linden_avatar>`, or a `<param>` (or its effect child) has a missing
    /// required attribute or a malformed number / vector / colour.
    pub fn from_xml(xml: &str) -> Result<Self, ParamError> {
        let doc = roxmltree::Document::parse(xml)?;
        let root = doc.root_element();
        if root.tag_name().name() != "linden_avatar" {
            return Err(ParamError::UnexpectedRoot {
                found: root.tag_name().name().to_owned(),
            });
        }

        // Collect every `<param>` anywhere in the document (== the viewer's load
        // order across sections), deduplicating by id with the last occurrence
        // winning, mirroring the reference viewer's `addVisualParam` map
        // overwrite. The final wire order is by id, so document order only
        // decides which definition wins a duplicate id.
        let mut by_id_raw: HashMap<i32, VisualParam> = HashMap::new();
        for node in doc
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "param")
        {
            let param = parse_param(node)?;
            by_id_raw.insert(param.id, param);
        }

        // Sort by ascending id and materialize the final vector.
        let mut ids: Vec<i32> = by_id_raw.keys().copied().collect();
        ids.sort_unstable();
        let mut params = Vec::with_capacity(ids.len());
        let mut by_id = HashMap::with_capacity(ids.len());
        let mut transmitted = Vec::new();
        for id in ids {
            if let Some(param) = by_id_raw.remove(&id) {
                let index = params.len();
                if param.is_transmitted() {
                    transmitted.push(index);
                }
                by_id.insert(id, index);
                params.push(param);
            }
        }

        Ok(Self {
            params,
            transmitted,
            by_id,
        })
    }

    /// All params, sorted by ascending id.
    #[must_use]
    pub fn all(&self) -> &[VisualParam] {
        &self.params
    }

    /// The total number of params in the table.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.params.len()
    }

    /// Whether the table has no params.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// The param with the given id, if present.
    #[must_use]
    pub fn get(&self, id: i32) -> Option<&VisualParam> {
        self.by_id
            .get(&id)
            .and_then(|&index| self.params.get(index))
    }

    /// The transmitted params, in wire order (ascending id) — one per byte of
    /// `AvatarAppearance.visual_params`.
    #[must_use]
    pub fn transmitted(&self) -> Vec<&VisualParam> {
        self.transmitted
            .iter()
            .filter_map(|&index| self.params.get(index))
            .collect()
    }

    /// The number of transmitted params — the expected length of a full
    /// `AvatarAppearance.visual_params` vector.
    #[must_use]
    pub const fn transmitted_count(&self) -> usize {
        self.transmitted.len()
    }

    /// Map an `AvatarAppearance.visual_params` byte vector onto typed weights.
    ///
    /// Byte `i` is dequantized against the `i`-th transmitted param (ascending
    /// id order). A vector shorter than [`Self::transmitted_count`] leaves the
    /// remaining params at their [`default`](VisualParam::default); extra bytes
    /// are ignored.
    #[must_use]
    pub fn map_appearance(&self, bytes: &[u8]) -> AppearanceValues {
        let mut values = Vec::with_capacity(self.transmitted.len());
        let mut by_id = HashMap::with_capacity(self.transmitted.len());
        for (slot, &index) in self.transmitted.iter().enumerate() {
            if let Some(param) = self.params.get(index) {
                let weight = match bytes.get(slot) {
                    Some(&byte) => param.weight_from_byte(byte),
                    None => param.default,
                };
                by_id.insert(param.id, weight);
                values.push(ParamValue {
                    id: param.id,
                    weight,
                    byte: bytes.get(slot).copied(),
                });
            }
        }
        AppearanceValues { values, by_id }
    }

    /// Build an `AvatarAppearance.visual_params` byte vector from a param-weight
    /// lookup — the inverse of [`map_appearance`](Self::map_appearance). One byte
    /// per transmitted param in wire order (ascending id), quantized against that
    /// param's `[min, max]`. A param the lookup returns `None` for falls back to
    /// its authored [`default`](VisualParam::default), so a partial source (e.g.
    /// only the worn Shape wearable's params) still yields a complete vector that
    /// is neutral where unset.
    #[must_use]
    pub fn encode_appearance(&self, weight_of: impl Fn(i32) -> Option<f32>) -> Vec<u8> {
        self.transmitted
            .iter()
            .filter_map(|&index| self.params.get(index))
            .map(|param| param.byte_from_weight(weight_of(param.id).unwrap_or(param.default)))
            .collect()
    }
}

/// The typed weights produced by mapping a wire appearance vector through a
/// [`VisualParams`] table.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceValues {
    /// One value per transmitted param, in wire order.
    values: Vec<ParamValue>,
    /// Lookup from a param id to its resolved weight.
    by_id: HashMap<i32, f32>,
}

impl AppearanceValues {
    /// The resolved weights, in wire order.
    #[must_use]
    pub fn values(&self) -> &[ParamValue] {
        &self.values
    }

    /// The number of resolved values (the transmitted-param count).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether there are no values.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The resolved weight for a given param id, if that param was transmitted.
    #[must_use]
    pub fn weight(&self, id: i32) -> Option<f32> {
        self.by_id.get(&id).copied()
    }
}

/// One resolved param weight from an appearance mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamValue {
    /// The param's id.
    pub id: i32,
    /// The dequantized weight (or the param default where the vector was short).
    pub weight: f32,
    /// The raw wire byte, or `None` if the vector was shorter than the
    /// transmitted-param count at this slot (weight is then the default).
    pub byte: Option<u8>,
}

/// Dequantize a wire byte into a weight, replicating Firestorm's `U8_to_F32`
/// (`llquantize.h`): a linear `[0, 255] → [lower, upper]` ramp with a
/// snap-to-zero within one quantization step so neutral values come through as
/// exactly `0`.
fn u8_to_f32(byte: u8, lower: f32, upper: f32) -> f32 {
    let delta = upper - lower;
    let val = f32::from(byte) / 255.0 * delta + lower;
    let max_error = delta.abs() / 255.0;
    if val.abs() < max_error { 0.0 } else { val }
}

/// Quantize a weight into a wire byte, the inverse of [`u8_to_f32`] — Firestorm's
/// `F32_to_U8` (`llquantize.h`): clamp the weight into the param range, then a
/// linear `[lower, upper] → [0, 255]` ramp, rounded. A zero-width range (a
/// non-slider param) encodes as `0`. Handles an inverted range (`min > max`, some
/// driven params) the same way [`u8_to_f32`] reads it.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the ramp is clamped to 0.0..=255.0 before truncation, so it fits u8"
)]
fn f32_to_u8(weight: f32, lower: f32, upper: f32) -> u8 {
    let delta = upper - lower;
    if delta == 0.0 {
        return 0;
    }
    let clamped = weight.clamp(lower.min(upper), lower.max(upper));
    ((clamped - lower) / delta * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// An RGBA byte quad as linear `0.0..=1.0` components (a [`ColorRamp`] stop).
fn rgba_to_unit(rgba: [u8; 4]) -> [f32; 4] {
    [
        f32::from(rgba[0]) / 255.0,
        f32::from(rgba[1]) / 255.0,
        f32::from(rgba[2]) / 255.0,
        f32::from(rgba[3]) / 255.0,
    ]
}

/// Widen a small `usize` (a colour-ramp stop count) to `f32`; ramp lengths are
/// tiny, far within the exact-integer range.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "colour-ramp stop counts are tiny, well within f32's exact-integer range"
)]
const fn f32_from_usize(value: usize) -> f32 {
    value as f32
}

/// Floor a non-negative `f32` (a clamped ramp index) to `usize`; a negative or
/// non-finite value maps to `0`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is a clamped, non-negative ramp index; its floor fits usize"
)]
fn usize_floor(value: f32) -> usize {
    if value.is_finite() && value >= 0.0 {
        value.floor() as usize
    } else {
        0
    }
}

/// Parse one `<param>` element into a [`VisualParam`].
fn parse_param(node: roxmltree::Node<'_, '_>) -> Result<VisualParam, ParamError> {
    let id = req_i32(node, "param", "id")?;
    let group = ParamGroup::from_code(opt_u32(node, "group")?.unwrap_or(0));
    let name = req_attr(node, "param", "name")?.to_owned();
    let label = node.attribute("label").map(str::to_owned);
    let wearable = node.attribute("wearable").map(str::to_owned);
    let sex = parse_sex(node.attribute("sex"));
    // value_min / value_max default to 0 / 1 in the reference viewer.
    let min = opt_f32(node, "value_min")?.unwrap_or(0.0);
    let max = opt_f32(node, "value_max")?.unwrap_or(1.0);
    let default = clamp(opt_f32(node, "value_default")?.unwrap_or(0.0), min, max);
    let effect = parse_effect(node, min, max)?;

    Ok(VisualParam {
        id,
        group,
        name,
        label,
        wearable,
        sex,
        min,
        max,
        default,
        effect,
    })
}

/// Determine a param's effect from its first recognized child element.
fn parse_effect(
    node: roxmltree::Node<'_, '_>,
    min: f32,
    max: f32,
) -> Result<ParamEffect, ParamError> {
    for child in node.children().filter(roxmltree::Node::is_element) {
        match child.tag_name().name() {
            "param_skeleton" => return Ok(ParamEffect::Skeleton(parse_bones(child)?)),
            "param_driver" => return Ok(ParamEffect::Driver(parse_driven(child, min, max)?)),
            "param_morph" => return Ok(ParamEffect::Morph),
            "param_color" => {
                return Ok(ParamEffect::Color(ColorRamp {
                    operation: ColorOp::from_operation(
                        child.attribute("operation").unwrap_or_default(),
                    ),
                    stops: parse_colors(child)?,
                }));
            }
            "param_alpha" => return Ok(ParamEffect::Alpha),
            _ => {}
        }
    }
    Ok(ParamEffect::None)
}

/// Parse the `<bone>` children of a `<param_skeleton>`. A bone missing its
/// (required) name or scale is skipped, matching the reference viewer.
fn parse_bones(node: roxmltree::Node<'_, '_>) -> Result<Vec<BoneOffset>, ParamError> {
    let mut bones = Vec::new();
    for child in node
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "bone")
    {
        let (Some(name), Some(scale)) = (child.attribute("name"), child.attribute("scale")) else {
            continue;
        };
        let scale = parse_vec3(scale, "scale")?;
        let offset = match child.attribute("offset") {
            Some(raw) => Some(parse_vec3(raw, "offset")?),
            None => None,
        };
        bones.push(BoneOffset {
            bone: name.to_owned(),
            scale,
            offset,
        });
    }
    Ok(bones)
}

/// Parse the `<driven>` children of a `<param_driver>`. Absent thresholds
/// default to the driver's own weight bounds.
fn parse_driven(
    node: roxmltree::Node<'_, '_>,
    min: f32,
    max: f32,
) -> Result<Vec<DrivenParam>, ParamError> {
    let mut driven = Vec::new();
    for child in node
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "driven")
    {
        let id = req_i32(child, "driven", "id")?;
        driven.push(DrivenParam {
            id,
            min1: opt_f32(child, "min1")?.unwrap_or(min),
            max1: opt_f32(child, "max1")?.unwrap_or(max),
            max2: opt_f32(child, "max2")?.unwrap_or(max),
            min2: opt_f32(child, "min2")?.unwrap_or(max),
        });
    }
    Ok(driven)
}

/// Parse the `<value color="r, g, b, a">` children of a `<param_color>`.
fn parse_colors(node: roxmltree::Node<'_, '_>) -> Result<Vec<[u8; 4]>, ParamError> {
    let mut colors = Vec::new();
    for child in node
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "value")
    {
        if let Some(raw) = child.attribute("color") {
            colors.push(parse_color(raw)?);
        }
    }
    Ok(colors)
}

/// Parse one `"r, g, b, a"` colour string into an RGBA quad.
fn parse_color(value: &str) -> Result<[u8; 4], ParamError> {
    let mut parts = value.split(',');
    let mut out = [0_u8; 4];
    for slot in &mut out {
        let token = parts.next().ok_or_else(|| ParamError::BadColor {
            value: value.to_owned(),
        })?;
        match token.trim().parse::<u8>() {
            Ok(parsed) => *slot = parsed,
            Err(_) => {
                return Err(ParamError::BadColor {
                    value: value.to_owned(),
                });
            }
        }
    }
    if parts.next().is_some() {
        return Err(ParamError::BadColor {
            value: value.to_owned(),
        });
    }
    Ok(out)
}

/// Map the optional `sex` attribute onto [`ParamSex`] (default [`Both`]).
///
/// [`Both`]: ParamSex::Both
fn parse_sex(value: Option<&str>) -> ParamSex {
    match value {
        Some("male") => ParamSex::Male,
        Some("female") => ParamSex::Female,
        _ => ParamSex::Both,
    }
}

/// Clamp `value` into `[min, max]` without the panic path of [`f32::clamp`]
/// (which asserts `min <= max`), so malformed bounds never panic.
fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Return a required attribute or a [`ParamError::MissingAttribute`].
fn req_attr<'a>(
    node: roxmltree::Node<'a, '_>,
    element: &'static str,
    attribute: &'static str,
) -> Result<&'a str, ParamError> {
    node.attribute(attribute)
        .ok_or(ParamError::MissingAttribute { element, attribute })
}

/// Parse a required `i32` attribute.
fn req_i32(
    node: roxmltree::Node<'_, '_>,
    element: &'static str,
    attribute: &'static str,
) -> Result<i32, ParamError> {
    let raw = req_attr(node, element, attribute)?;
    raw.parse::<i32>().map_err(|_err| ParamError::BadNumber {
        attribute,
        value: raw.to_owned(),
    })
}

/// Parse an optional `u32` attribute (absent → `Ok(None)`).
fn opt_u32(
    node: roxmltree::Node<'_, '_>,
    attribute: &'static str,
) -> Result<Option<u32>, ParamError> {
    match node.attribute(attribute) {
        None => Ok(None),
        Some(raw) => raw
            .parse::<u32>()
            .map(Some)
            .map_err(|_err| ParamError::BadNumber {
                attribute,
                value: raw.to_owned(),
            }),
    }
}

/// Parse an optional `f32` attribute (absent → `Ok(None)`).
fn opt_f32(
    node: roxmltree::Node<'_, '_>,
    attribute: &'static str,
) -> Result<Option<f32>, ParamError> {
    match node.attribute(attribute) {
        None => Ok(None),
        Some(raw) => raw
            .parse::<f32>()
            .map(Some)
            .map_err(|_err| ParamError::BadNumber {
                attribute,
                value: raw.to_owned(),
            }),
    }
}

/// Parse exactly three space-separated `f32`s from `value`.
fn parse_vec3(value: &str, attribute: &'static str) -> Result<[f32; 3], ParamError> {
    let mut parts = value.split_whitespace();
    let mut out = [0.0_f32; 3];
    for slot in &mut out {
        let token = parts.next().ok_or_else(|| ParamError::BadVector {
            attribute,
            value: value.to_owned(),
        })?;
        match token.parse::<f32>() {
            Ok(parsed) => *slot = parsed,
            Err(_) => {
                return Err(ParamError::BadVector {
                    attribute,
                    value: value.to_owned(),
                });
            }
        }
    }
    if parts.next().is_some() {
        return Err(ParamError::BadVector {
            attribute,
            value: value.to_owned(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{AppearanceValues, ParamEffect, ParamError, ParamGroup, ParamSex, VisualParams};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A minimal committed visual-param fixture: one param of each effect type,
    /// one non-transmitted (group 1) param, ids deliberately out of document
    /// order (1, 10, 32, 111, 112, 4) so the id sort is exercised.
    const MINI_PARAMS: &str = include_str!("../tests/fixtures/mini_params.xml");

    /// Compare two floats within a tolerance (keeps the assertion off
    /// `float_cmp`).
    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
    }

    #[test]
    fn parses_table_and_sorts_by_id() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        // Six params total, sorted by ascending id regardless of document order.
        let ids: Vec<i32> = params.all().iter().map(|param| param.id).collect();
        assert_eq!(ids, [1, 4, 10, 32, 111, 112]);
        assert_eq!(params.len(), 6);
        assert!(!params.is_empty());

        // Metadata on a representative param.
        let male = params.get(32).ok_or("param 32 present")?;
        assert_eq!(male.name, "Male_Skeleton");
        assert_eq!(male.group, ParamGroup::Tweakable);
        assert_eq!(male.sex, ParamSex::Male);
        assert_eq!(male.wearable.as_deref(), Some("shape"));
        assert!(approx(male.min, 0.0));
        assert!(approx(male.max, 1.0));
        Ok(())
    }

    #[test]
    fn transmitted_subset_is_wire_order() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        // Group-1 param 10 is excluded; the rest are wire-ordered by id.
        let wire: Vec<i32> = params.transmitted().iter().map(|param| param.id).collect();
        assert_eq!(wire, [1, 4, 32, 111, 112]);
        assert_eq!(params.transmitted_count(), 5);
        assert!(matches!(
            params.get(10).map(|param| param.group),
            Some(ParamGroup::Animatable)
        ));
        assert_eq!(
            params.get(10).map(super::VisualParam::is_transmitted),
            Some(false)
        );
        Ok(())
    }

    #[test]
    fn parses_each_effect_type() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;

        // Skeleton: two bones, the second carrying an offset.
        let ParamEffect::Skeleton(bones) = &params.get(32).ok_or("32")?.effect else {
            return Err("param 32 is skeletal".into());
        };
        assert_eq!(bones.len(), 2);
        assert_eq!(bones.first().ok_or("bone 0")?.bone, "mNeck");
        assert!(approx(bones.first().ok_or("bone 0")?.scale[2], 0.2));
        assert_eq!(bones.first().ok_or("bone 0")?.offset, None);
        let head = bones.get(1).ok_or("bone 1")?;
        assert_eq!(head.bone, "mHead");
        assert!(head.offset.is_some_and(|off| approx(off[2], 0.05)));

        // Morph carries no data; its target is resolved by name later.
        assert_eq!(
            params.get(1).map(|param| &param.effect),
            Some(&ParamEffect::Morph)
        );

        // Colour ramp.
        let ParamEffect::Color(ramp) = &params.get(111).ok_or("111")?.effect else {
            return Err("param 111 is colour".into());
        };
        assert_eq!(ramp.stops, [[252, 215, 200, 255], [90, 40, 16, 255]]);
        assert_eq!(ramp.operation, super::ColorOp::Add);

        // Alpha.
        assert_eq!(
            params.get(112).map(|param| &param.effect),
            Some(&ParamEffect::Alpha)
        );

        // Driver: first driven inherits the driver's bounds, second is explicit.
        let ParamEffect::Driver(driven) = &params.get(4).ok_or("4")?.effect else {
            return Err("param 4 is a driver".into());
        };
        assert_eq!(driven.len(), 2);
        let first = driven.first().ok_or("driven 0")?;
        assert_eq!(first.id, 1);
        assert!(approx(first.min1, -0.8) && approx(first.max1, 2.5));
        assert!(approx(first.max2, 2.5) && approx(first.min2, 2.5));
        let second = driven.get(1).ok_or("driven 1")?;
        assert_eq!(second.id, 112);
        assert!(approx(second.min1, -2.0) && approx(second.min2, 0.0));
        Ok(())
    }

    #[test]
    fn maps_appearance_bytes_to_weights() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        // Wire order [1, 4, 32, 111, 112].
        let values: AppearanceValues = params.map_appearance(&[255, 0, 128, 255, 0]);
        assert_eq!(values.len(), 5);

        // id 1: byte 255 -> max 2.0; id 4: byte 0 -> min -0.8.
        assert!(values.weight(1).is_some_and(|w| approx(w, 2.0)));
        assert!(values.weight(4).is_some_and(|w| approx(w, -0.8)));
        // id 32: byte 128 over [0, 1] -> ~0.502.
        assert!(values.weight(32).is_some_and(|w| approx(w, 0.501_96)));
        assert!(values.weight(111).is_some_and(|w| approx(w, 1.0)));
        assert!(values.weight(112).is_some_and(|w| approx(w, 0.0)));

        // The first value records its raw byte, in wire order.
        assert_eq!(values.values().first().map(|v| v.byte), Some(Some(255)));
        assert_eq!(values.values().first().map(|v| v.id), Some(1));
        Ok(())
    }

    #[test]
    fn encode_appearance_round_trips() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        // Supply weights for two transmitted params; the rest fall back to their
        // (zero) defaults — exactly how a partial worn-Shape source is completed.
        let bytes = params.encode_appearance(|id| match id {
            1 => Some(1.0),   // over [-0.3, 2.0]
            32 => Some(0.75), // over [0, 1]
            _ => None,
        });
        // One byte per transmitted param, in wire order [1, 4, 32, 111, 112].
        assert_eq!(bytes.len(), params.transmitted_count());

        // Decoding reproduces the supplied weights (within one quantization step,
        // ~range/255) and leaves the unset params at their defaults.
        let within = |a: f32, b: f32| (a - b).abs() < 0.01;
        let values = params.map_appearance(&bytes);
        assert!(values.weight(1).is_some_and(|w| within(w, 1.0)));
        assert!(values.weight(32).is_some_and(|w| within(w, 0.75)));
        assert_eq!(values.weight(4), Some(0.0));
        assert_eq!(values.weight(111), Some(0.0));
        assert_eq!(values.weight(112), Some(0.0));
        Ok(())
    }

    #[test]
    fn snap_to_zero_within_one_step() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        // Byte 33 over [-0.3, 2] lands within one quantization step of zero, so
        // the reference viewer's U8_to_F32 snaps it to exactly 0.
        let values = params.map_appearance(&[33]);
        assert_eq!(values.weight(1), Some(0.0));
        Ok(())
    }

    #[test]
    fn short_vector_uses_defaults() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        // Only two bytes: the remaining three transmitted params fall back to
        // their defaults (all 0 here) with no raw byte recorded.
        let values = params.map_appearance(&[255, 0]);
        assert_eq!(values.len(), 5);
        assert!(values.weight(1).is_some_and(|w| approx(w, 2.0)));
        assert_eq!(values.weight(32), Some(0.0));
        assert_eq!(values.weight(111), Some(0.0));
        // id 32 sits at slot 2, past the two supplied bytes -> byte is None.
        let slot32 = values
            .values()
            .iter()
            .find(|value| value.id == 32)
            .ok_or("32 present")?;
        assert_eq!(slot32.byte, None);
        Ok(())
    }

    #[test]
    fn empty_vector_is_all_defaults() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        let values = params.map_appearance(&[]);
        assert_eq!(values.len(), 5);
        assert!(values.values().iter().all(|value| value.byte.is_none()));
        Ok(())
    }

    #[test]
    fn rejects_wrong_root() {
        let result = VisualParams::from_xml("<linden_skeleton/>");
        assert!(matches!(result, Err(ParamError::UnexpectedRoot { .. })));
    }

    #[test]
    fn group_code_mapping() {
        assert_eq!(ParamGroup::from_code(0), ParamGroup::Tweakable);
        assert_eq!(ParamGroup::from_code(1), ParamGroup::Animatable);
        assert_eq!(ParamGroup::from_code(2), ParamGroup::TweakableNoTransmit);
        assert_eq!(ParamGroup::from_code(3), ParamGroup::TransmitNotTweakable);
        // Out-of-range defaults to tweakable, as the reference viewer does.
        assert_eq!(ParamGroup::from_code(9), ParamGroup::Tweakable);
        assert!(ParamGroup::Tweakable.is_transmitted());
        assert!(ParamGroup::TransmitNotTweakable.is_transmitted());
        assert!(!ParamGroup::Animatable.is_transmitted());
        assert!(!ParamGroup::TweakableNoTransmit.is_transmitted());
    }
}
