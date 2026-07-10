//! The per-region **bake plan**: which worn-wearable texture layers feed each
//! base-body bake, in composite order, and how each is tinted (P15.2).
//!
//! The reference viewer's full `LLTexLayerSet` is far richer than the P15.1
//! compositor models — it also carries procedural cosmetic layers (skin shading,
//! lipstick, blush, freckles, bump maps) driven purely by visual params with no
//! wearable texture. Those need a per-param procedural renderer the simplified
//! [`composite`](crate::composite) engine does not have, so the plan here keeps
//! the layers backed by a **worn wearable's texture** (the skin bodypaint,
//! clothing, tattoos, alpha masks) and the **static `character/` TGA** diffuse
//! layers the reference viewer bakes into every avatar regardless of wearables —
//! the skin-grain base, the eye sclera (`eyewhite.tga`), and the skin colour
//! details (nipples / toenails / face colour). The purely procedural cosmetic /
//! bump layers (shading, highlights, lipstick, blush, freckles) are still out of
//! scope: they need the per-param colour renderer this crate does not have.
//! Layer order and membership follow `avatar_lad.xml`'s `<layer_set>` blocks.
//!
//! [`region_layers`] turns a plan into the ordered [`Layer`] list the compositor
//! wants, given closures that resolve each slot's decoded texture and each
//! layer's tint from the worn wearable assets (the runtime crates own the fetch /
//! decode / param lookup; this stays I/O-free).

use crate::composite::{Layer, LayerKind, ShapeMask, TexGen};
use crate::region::BakeRegion;
use sl_proto::WearableType;
use sl_proto::avatar_texture as tex;
use sl_texture::DecodedImage;

/// Where a planned layer's tint comes from. The runtime resolves it against the
/// worn wearable's visual params (`sl-avatar`'s `bakecolor`); this crate only
/// records which source each layer uses so it stays decoupled from param
/// evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerTint {
    /// No tint — the texture carries its own colour (opaque white multiply).
    White,
    /// A named `<global_color>` (`skin_color` / `hair_color` / `eye_color`), the
    /// skin / hair / eye base colour from the body-part's colour params.
    Global(&'static str),
    /// The layer's own inline colour params (their ids), a clothing / tattoo
    /// red-green-blue tint.
    Params(&'static [i32]),
}

/// Where a planned layer's texture comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerSource {
    /// No texture — a solid fill of the layer's [`tint`](PlannedLayer::tint) (the
    /// skin-tone base, or a static base whose TGA could not be loaded).
    Solid,
    /// A worn wearable's `TextureEntry` layer slot (`sl-proto`'s
    /// `avatar_texture` indices) supplies the texture.
    Wearable(usize),
    /// A static `character/` TGA file supplies the texture — the reference
    /// viewer's baked-in skin-grain / sclera / colour-detail layers, present on
    /// every avatar regardless of the worn wearables.
    Static(&'static str),
}

/// One garment-shape alpha mask a clothing layer is bounded by (R14): the
/// `avatar_lad.xml` `<param_alpha>` static TGA, the visual-param that drives its
/// weight, and how it processes / combines. The reference viewer stacks these to
/// carve a garment's `local_texture` down to its garment extent (sleeve length,
/// shirt bottom, collar, pants length, waist, …) so a solid-fabric shirt or pants
/// does not paint the bare hands and feet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeMaskSpec {
    /// The visual-param id whose weight drives this mask (e.g. `600`, sleeve
    /// length cloth). Resolved against the worn wearable's params.
    pub param_id: i32,
    /// The static `character/` grayscale mask TGA (e.g. `shirt_sleeve_alpha.tga`).
    pub tga: &'static str,
    /// Whether the mask multiplies (min) or adds (max) into the layer's coverage,
    /// the `param_alpha` `multiply_blend`.
    pub multiply_blend: bool,
    /// The LUT ramp width (`0` is a hard threshold), the `param_alpha` `domain`.
    pub domain: f32,
}

/// One planned layer of a bake region: where its texture comes from, which
/// wearable it belongs to, how it composites, and how it is tinted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlannedLayer {
    /// Where this layer's texture comes from.
    pub source: LayerSource,
    /// The wearable type that supplies this layer (decides whether it is worn and
    /// which wearable's params tint it). For a [`LayerSource::Static`] layer this
    /// is the region's owning body part (skin / eyes), used only for the tint /
    /// base worn-gate.
    pub wearable: WearableType,
    /// How this layer composites over the ones below it.
    pub kind: LayerKind,
    /// How the layer texture maps onto the body.
    pub tex_gen: TexGen,
    /// Where this layer's tint comes from.
    pub tint: LayerTint,
    /// The garment-shape masks bounding this clothing layer to its garment extent
    /// (R14); empty for a non-garment layer (skin base, tattoo, alpha).
    pub masks: &'static [ShapeMaskSpec],
}

impl PlannedLayer {
    /// A textured layer of `kind` from `wearable`'s `slot`, tinted by `tint`.
    const fn textured(
        slot: usize,
        wearable: WearableType,
        kind: LayerKind,
        tint: LayerTint,
    ) -> Self {
        Self {
            source: LayerSource::Wearable(slot),
            wearable,
            kind,
            tex_gen: TexGen::Default,
            tint,
            masks: &[],
        }
    }

    /// A clothing garment layer ([`LayerKind::Blend`]) from `wearable`'s `slot`,
    /// tinted by `tint` and bounded to its garment extent by `masks` (R14) — the
    /// `avatar_lad.xml` `<param_alpha>` shape masks that keep the fabric off the
    /// bare hands / feet.
    const fn garment(
        slot: usize,
        wearable: WearableType,
        tint: LayerTint,
        masks: &'static [ShapeMaskSpec],
    ) -> Self {
        Self {
            source: LayerSource::Wearable(slot),
            wearable,
            kind: LayerKind::Blend,
            tex_gen: TexGen::Default,
            tint,
            masks,
        }
    }

    /// A static-TGA base layer from `file` (a `character/` TGA), attributed to
    /// `wearable` (for the base worn-gate) and tinted by `tint` — the skin-grain
    /// foundation, or the eye sclera. Falls back to a solid tint fill if the TGA
    /// cannot be loaded, so the region never loses its base.
    const fn static_base(file: &'static str, wearable: WearableType, tint: LayerTint) -> Self {
        Self {
            source: LayerSource::Static(file),
            wearable,
            kind: LayerKind::Base,
            tex_gen: TexGen::Default,
            tint,
            masks: &[],
        }
    }

    /// A static-TGA layer of `kind` from `file` (a `character/` TGA), attributed
    /// to `wearable` and tinted by `tint` — the reference viewer's baked-in skin
    /// colour details (nipples / toenails / face colour). Skipped if the TGA
    /// cannot be loaded.
    const fn static_layer(
        file: &'static str,
        wearable: WearableType,
        kind: LayerKind,
        tint: LayerTint,
    ) -> Self {
        Self {
            source: LayerSource::Static(file),
            wearable,
            kind,
            tex_gen: TexGen::Default,
            tint,
            masks: &[],
        }
    }
}

/// The shirt / undershirt shape masks (`avatar_lad.xml` `upper_clothes` /
/// `upper_undershirt` layers): sleeve length (additive), then shirt bottom and
/// the front / back collar (multiplicative). The sleeve mask is what keeps the
/// fabric off the forearms and hands.
const SHIRT_MASKS: &[ShapeMaskSpec] = &[
    ShapeMaskSpec {
        param_id: 600,
        tga: "shirt_sleeve_alpha.tga",
        multiply_blend: false,
        domain: 0.01,
    },
    ShapeMaskSpec {
        param_id: 601,
        tga: "shirt_bottom_alpha.tga",
        multiply_blend: true,
        domain: 0.05,
    },
    ShapeMaskSpec {
        param_id: 602,
        tga: "shirt_collar_alpha.tga",
        multiply_blend: true,
        domain: 0.05,
    },
    ShapeMaskSpec {
        param_id: 778,
        tga: "shirt_collar_back_alpha.tga",
        multiply_blend: true,
        domain: 0.05,
    },
];

/// The undershirt shape masks — the same TGAs as the shirt, driven by the
/// undershirt's own sleeve / bottom / collar params.
const UNDERSHIRT_MASKS: &[ShapeMaskSpec] = &[
    ShapeMaskSpec {
        param_id: 1042,
        tga: "shirt_sleeve_alpha.tga",
        multiply_blend: false,
        domain: 0.01,
    },
    ShapeMaskSpec {
        param_id: 1044,
        tga: "shirt_bottom_alpha.tga",
        multiply_blend: true,
        domain: 0.05,
    },
    ShapeMaskSpec {
        param_id: 1046,
        tga: "shirt_collar_alpha.tga",
        multiply_blend: true,
        domain: 0.05,
    },
    ShapeMaskSpec {
        param_id: 1048,
        tga: "shirt_collar_back_alpha.tga",
        multiply_blend: true,
        domain: 0.05,
    },
];

/// The glove shape masks (`upper_gloves`): glove length (additive), then the
/// finger cut-out (multiplicative). Bounds the gloves to the hands / forearms.
const GLOVES_MASKS: &[ShapeMaskSpec] = &[
    ShapeMaskSpec {
        param_id: 1058,
        tga: "glove_length_alpha.tga",
        multiply_blend: false,
        domain: 0.01,
    },
    ShapeMaskSpec {
        param_id: 1060,
        tga: "gloves_fingers_alpha.tga",
        multiply_blend: true,
        domain: 0.01,
    },
];

/// The upper-jacket shape masks (`upper_jacket`): sleeve length (additive), then
/// collar front / back, bottom length and the open front (multiplicative).
const UPPER_JACKET_MASKS: &[ShapeMaskSpec] = &[
    ShapeMaskSpec {
        param_id: 1020,
        tga: "shirt_sleeve_alpha.tga",
        multiply_blend: false,
        domain: 0.01,
    },
    ShapeMaskSpec {
        param_id: 1022,
        tga: "shirt_collar_alpha.tga",
        multiply_blend: true,
        domain: 0.05,
    },
    ShapeMaskSpec {
        param_id: 1024,
        tga: "shirt_collar_back_alpha.tga",
        multiply_blend: true,
        domain: 0.05,
    },
    ShapeMaskSpec {
        param_id: 620,
        tga: "jacket_length_upper_alpha.tga",
        multiply_blend: true,
        domain: 0.01,
    },
    ShapeMaskSpec {
        param_id: 622,
        tga: "jacket_open_upper_alpha.tga",
        multiply_blend: true,
        domain: 0.01,
    },
];

/// The underpants shape masks (`lower_underpants`): pants length (additive) then
/// waist (additive).
const UNDERPANTS_MASKS: &[ShapeMaskSpec] = &[
    ShapeMaskSpec {
        param_id: 1054,
        tga: "pants_length_alpha.tga",
        multiply_blend: false,
        domain: 0.01,
    },
    ShapeMaskSpec {
        param_id: 1056,
        tga: "pants_waist_alpha.tga",
        multiply_blend: false,
        domain: 0.05,
    },
];

/// The socks shape mask (`lower_socks`): sock length, bounding the socks to the
/// feet / lower legs.
const SOCKS_MASKS: &[ShapeMaskSpec] = &[ShapeMaskSpec {
    param_id: 1050,
    tga: "shoe_height_alpha.tga",
    multiply_blend: false,
    domain: 0.01,
}];

/// The shoes shape mask (`lower_shoes`): shoe height.
const SHOES_MASKS: &[ShapeMaskSpec] = &[ShapeMaskSpec {
    param_id: 1052,
    tga: "shoe_height_alpha.tga",
    multiply_blend: false,
    domain: 0.01,
}];

/// The pants shape masks (`lower_clothes`): waist height then pants length (both
/// additive). The length mask is what keeps the fabric off the bare feet.
const PANTS_MASKS: &[ShapeMaskSpec] = &[
    ShapeMaskSpec {
        param_id: 614,
        tga: "pants_waist_alpha.tga",
        multiply_blend: false,
        domain: 0.05,
    },
    ShapeMaskSpec {
        param_id: 615,
        tga: "pants_length_alpha.tga",
        multiply_blend: false,
        domain: 0.01,
    },
];

/// The lower-jacket shape masks (`lower_jacket`): bottom length (additive) then
/// the open front (multiplicative).
const LOWER_JACKET_MASKS: &[ShapeMaskSpec] = &[
    ShapeMaskSpec {
        param_id: 621,
        tga: "jacket_length_lower_alpha.tga",
        multiply_blend: false,
        domain: 0.01,
    },
    ShapeMaskSpec {
        param_id: 623,
        tga: "jacket_open_lower_alpha.tga",
        multiply_blend: true,
        domain: 0.01,
    },
];

/// Shorthands for the three composite kinds, keeping the tables compact.
use LayerKind::{AlphaMask, Base, Blend};
use WearableType::{
    Alpha, Eyes, Gloves, Hair, Jacket, Pants, Shirt, Shoes, Skin, Skirt, Socks, Tattoo, Underpants,
    Undershirt, Universal,
};

/// The head bake's layers, in composite order (skin-grain base → face colour →
/// skin paint → alpha → tattoos), following `avatar_lad.xml`'s `head`
/// `<layer_set>` (the `base` skin-grain and `headcolor` static TGAs, minus the
/// procedural shading / make-up layers).
const HEAD: &[PlannedLayer] = &[
    PlannedLayer::static_base("head_skingrain.tga", Skin, LayerTint::Global("skin_color")),
    PlannedLayer::static_layer("head_color.tga", Skin, Blend, LayerTint::White),
    PlannedLayer::textured(tex::HEAD_BODYPAINT, Skin, Blend, LayerTint::White),
    // The eyelash-shape alpha (`avatar_lad.xml`'s `head` `eyelash alpha`
    // visibility-mask layer): carves the lash surround out of the head bake's
    // alpha so the eyelash mesh — which shares the head bake — renders the lashes
    // over a transparent surround rather than an opaque skin-coloured quad. It is
    // near-fully-opaque everywhere but the eyelash UV corner, so the head skin
    // stays opaque.
    PlannedLayer::static_layer("head_alpha.tga", Skin, AlphaMask, LayerTint::White),
    PlannedLayer::textured(tex::HEAD_ALPHA, Alpha, AlphaMask, LayerTint::White),
    PlannedLayer::textured(
        tex::HEAD_TATTOO,
        Tattoo,
        Blend,
        LayerTint::Params(&[1062, 1063, 1064]),
    ),
    PlannedLayer::textured(
        tex::HEAD_UNIVERSAL_TATTOO,
        Universal,
        Blend,
        LayerTint::Params(&[1229, 1230, 1231]),
    ),
];

/// The upper-body bake's layers, in composite order.
const UPPER: &[PlannedLayer] = &[
    PlannedLayer::static_base("body_skingrain.tga", Skin, LayerTint::Global("skin_color")),
    PlannedLayer::static_layer("upperbody_color.tga", Skin, Blend, LayerTint::White),
    PlannedLayer::textured(tex::UPPER_BODYPAINT, Skin, Blend, LayerTint::White),
    PlannedLayer::textured(
        tex::UPPER_TATTOO,
        Tattoo,
        Blend,
        LayerTint::Params(&[1065, 1066, 1067]),
    ),
    PlannedLayer::textured(
        tex::UPPER_UNIVERSAL_TATTOO,
        Universal,
        Blend,
        LayerTint::Params(&[1232, 1233, 1234]),
    ),
    PlannedLayer::garment(
        tex::UPPER_UNDERSHIRT,
        Undershirt,
        LayerTint::Params(&[821, 822, 823]),
        UNDERSHIRT_MASKS,
    ),
    PlannedLayer::garment(
        tex::UPPER_GLOVES,
        Gloves,
        LayerTint::Params(&[827, 829, 830]),
        GLOVES_MASKS,
    ),
    PlannedLayer::garment(
        tex::UPPER_SHIRT,
        Shirt,
        LayerTint::Params(&[803, 804, 805]),
        SHIRT_MASKS,
    ),
    PlannedLayer::garment(
        tex::UPPER_JACKET,
        Jacket,
        LayerTint::Params(&[831, 832, 833]),
        UPPER_JACKET_MASKS,
    ),
    PlannedLayer::textured(tex::UPPER_ALPHA, Alpha, AlphaMask, LayerTint::White),
];

/// The lower-body bake's layers, in composite order.
const LOWER: &[PlannedLayer] = &[
    PlannedLayer::static_base("body_skingrain.tga", Skin, LayerTint::Global("skin_color")),
    PlannedLayer::static_layer("lowerbody_color.tga", Skin, Blend, LayerTint::White),
    PlannedLayer::textured(tex::LOWER_BODYPAINT, Skin, Blend, LayerTint::White),
    PlannedLayer::textured(
        tex::LOWER_TATTOO,
        Tattoo,
        Blend,
        LayerTint::Params(&[1068, 1069, 1070]),
    ),
    PlannedLayer::textured(
        tex::LOWER_UNIVERSAL_TATTOO,
        Universal,
        Blend,
        LayerTint::Params(&[1235, 1236, 1237]),
    ),
    PlannedLayer::garment(
        tex::LOWER_UNDERPANTS,
        Underpants,
        LayerTint::Params(&[824, 825, 826]),
        UNDERPANTS_MASKS,
    ),
    PlannedLayer::garment(
        tex::LOWER_SOCKS,
        Socks,
        LayerTint::Params(&[818, 819, 820]),
        SOCKS_MASKS,
    ),
    PlannedLayer::garment(
        tex::LOWER_SHOES,
        Shoes,
        LayerTint::Params(&[812, 813, 817]),
        SHOES_MASKS,
    ),
    PlannedLayer::garment(
        tex::LOWER_PANTS,
        Pants,
        LayerTint::Params(&[806, 807, 808]),
        PANTS_MASKS,
    ),
    PlannedLayer::garment(
        tex::LOWER_JACKET,
        Jacket,
        LayerTint::Params(&[809, 810, 811]),
        LOWER_JACKET_MASKS,
    ),
    PlannedLayer::textured(tex::LOWER_ALPHA, Alpha, AlphaMask, LayerTint::White),
];

/// The eyes bake's layers: the opaque white sclera base (`eyewhite.tga`), the
/// iris blended over it (tinted by eye colour), its alpha, and a universal tattoo
/// overlay. The sclera base is what gives the eyeball its white — without it the
/// iris (a transparent-surround texture) reads as an untextured blob.
const EYES: &[PlannedLayer] = &[
    PlannedLayer::static_base("eyewhite.tga", Eyes, LayerTint::White),
    PlannedLayer::textured(tex::EYES_IRIS, Eyes, Blend, LayerTint::Global("eye_color")),
    PlannedLayer::textured(tex::EYES_ALPHA, Alpha, AlphaMask, LayerTint::White),
    PlannedLayer::textured(
        tex::EYES_TATTOO,
        Universal,
        Blend,
        LayerTint::Params(&[924, 925, 926]),
    ),
];

/// The skirt bake's layers: the skirt fabric (tinted by its colour params) and a
/// universal tattoo overlay.
const SKIRT_LAYERS: &[PlannedLayer] = &[
    PlannedLayer::textured(tex::SKIRT, Skirt, Base, LayerTint::Params(&[921, 922, 923])),
    PlannedLayer::textured(
        tex::SKIRT_TATTOO,
        Universal,
        Blend,
        LayerTint::Params(&[1208, 1209, 1210]),
    ),
];

/// The hair bake's layers: the hair texture (tinted by hair colour), its alpha,
/// and a universal tattoo overlay.
const HAIR_LAYERS: &[PlannedLayer] = &[
    PlannedLayer::textured(tex::HAIR, Hair, Base, LayerTint::Global("hair_color")),
    PlannedLayer::textured(tex::HAIR_ALPHA, Alpha, AlphaMask, LayerTint::White),
    PlannedLayer::textured(
        tex::HAIR_TATTOO,
        Universal,
        Blend,
        LayerTint::Params(&[1211, 1212, 1213]),
    ),
];

/// A "universal" bake's layers: a single universal-tattoo layer for that slot,
/// blended over nothing (the bake is transparent where the tattoo does not cover,
/// which is correct — a mesh body samples it only for the tattoo). Following the
/// reference viewer's `BakedEntry`s for `BAKED_LEFT_ARM`/`LEFT_LEG`/`AUX1-3`, each
/// of which lists exactly one `WT_UNIVERSAL` layer. No worn universal wearable ⇒
/// no layer ⇒ the region is skipped (never an all-transparent published bake).
///
/// The tattoo is tinted by its `avatar_lad.xml` `tattoo_<slot>_{red,green,blue}`
/// colour params (leftarm `1214-1216`, leftleg `1217-1219`, aux1 `1220-1222`, aux2
/// `1223-1225`, aux3 `1226-1228`), as the other universal-tattoo layers are.
const LEFT_ARM: &[PlannedLayer] = &[PlannedLayer::textured(
    tex::LEFT_ARM_TATTOO,
    Universal,
    Blend,
    LayerTint::Params(&[1214, 1215, 1216]),
)];

/// The universal left-leg bake's layers (see [`LEFT_ARM`]).
const LEFT_LEG: &[PlannedLayer] = &[PlannedLayer::textured(
    tex::LEFT_LEG_TATTOO,
    Universal,
    Blend,
    LayerTint::Params(&[1217, 1218, 1219]),
)];

/// The universal aux1 bake's layers (see [`LEFT_ARM`]).
const AUX1: &[PlannedLayer] = &[PlannedLayer::textured(
    tex::AUX1_TATTOO,
    Universal,
    Blend,
    LayerTint::Params(&[1220, 1221, 1222]),
)];

/// The universal aux2 bake's layers (see [`LEFT_ARM`]).
const AUX2: &[PlannedLayer] = &[PlannedLayer::textured(
    tex::AUX2_TATTOO,
    Universal,
    Blend,
    LayerTint::Params(&[1223, 1224, 1225]),
)];

/// The universal aux3 bake's layers (see [`LEFT_ARM`]).
const AUX3: &[PlannedLayer] = &[PlannedLayer::textured(
    tex::AUX3_TATTOO,
    Universal,
    Blend,
    LayerTint::Params(&[1226, 1227, 1228]),
)];

/// The ordered planned layers for a bake region, in composite order (bottom to
/// top). These describe *which* worn-wearable textures feed the region and how;
/// [`region_layers`] resolves them into concrete [`Layer`]s.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `region_plan` reads clearly"
)]
pub const fn region_plan(region: BakeRegion) -> &'static [PlannedLayer] {
    match region {
        BakeRegion::Head => HEAD,
        BakeRegion::UpperBody => UPPER,
        BakeRegion::LowerBody => LOWER,
        BakeRegion::Eyes => EYES,
        BakeRegion::Skirt => SKIRT_LAYERS,
        BakeRegion::Hair => HAIR_LAYERS,
        BakeRegion::LeftArm => LEFT_ARM,
        BakeRegion::LeftLeg => LEFT_LEG,
        BakeRegion::Aux1 => AUX1,
        BakeRegion::Aux2 => AUX2,
        BakeRegion::Aux3 => AUX3,
    }
}

/// Assemble the ordered [`Layer`] list a bake region composites from, resolving
/// each planned layer against the worn wearables and the static `character/`
/// TGAs.
///
/// - `worn(type)` — whether a wearable of that type is worn (gates every base
///   layer, which is the region's opaque foundation).
/// - `image_for(slot)` — the decoded texture for a `TextureEntry` layer slot, or
///   `None` when no worn wearable supplies it (that layer is then omitted).
/// - `static_image(file)` — the decoded static `character/` TGA of that name, or
///   `None` when it could not be loaded (a static base then falls back to a solid
///   tint fill; a static detail layer is omitted; a garment-shape mask whose TGA
///   is absent is simply not applied).
/// - `tint_for(tint, wearable)` — the linear-RGBA tint for a layer, resolved from
///   the wearable's visual params (opaque white for [`LayerTint::White`]).
/// - `mask_weight(param_id, wearable)` — the worn wearable's weight for a
///   garment-shape mask's driving param (the raw param value, e.g. sleeve length),
///   falling back to the param's default when the wearable does not set it (R14).
///
/// A worn-wearable layer whose `image_for` yields `None` is skipped; a base layer
/// is skipped unless its wearable is worn. A clothing layer additionally carries
/// its garment-shape masks ([`ShapeMaskSpec`]), resolved here into [`ShapeMask`]s
/// that bound the fabric to the garment extent. The result feeds
/// [`composite_region`](crate::composite_region).
pub fn region_layers(
    region: BakeRegion,
    worn: impl Fn(WearableType) -> bool,
    image_for: impl Fn(usize) -> Option<DecodedImage>,
    static_image: impl Fn(&str) -> Option<DecodedImage>,
    tint_for: impl Fn(LayerTint, WearableType) -> [f32; 4],
    mask_weight: impl Fn(i32, WearableType) -> f32,
) -> Vec<Layer> {
    let mut layers = Vec::new();
    for planned in region_plan(region) {
        // A base layer (the region's opaque foundation — skin grain, eye sclera)
        // renders only when its wearable is worn, matching the reference layer-set.
        if planned.kind == LayerKind::Base && !worn(planned.wearable) {
            continue;
        }
        let tint = tint_for(planned.tint, planned.wearable);
        let image = match planned.source {
            LayerSource::Solid => None,
            // A worn-wearable layer with no decoded texture contributes nothing.
            LayerSource::Wearable(slot) => match image_for(slot) {
                Some(image) => Some(image),
                None => continue,
            },
            LayerSource::Static(file) => static_image(file),
        };
        let layer = match image {
            Some(image) => {
                let base = match planned.kind {
                    LayerKind::Base => Layer::base(image),
                    LayerKind::Blend => Layer::blend(image),
                    LayerKind::AlphaMask => Layer::alpha_mask(image),
                };
                let masks = resolve_masks(planned, &static_image, &mask_weight);
                base.with_tint(tint)
                    .with_tex_gen(planned.tex_gen)
                    .with_masks(masks)
            }
            // No image: only a base falls back to a solid tint fill (the skin-tone
            // foundation, or a static base whose TGA was unavailable); a
            // detail / mask layer with no texture is dropped.
            None => {
                if planned.kind != LayerKind::Base {
                    continue;
                }
                Layer::solid(LayerKind::Base, tint).with_tex_gen(planned.tex_gen)
            }
        };
        layers.push(layer);
    }
    layers
}

/// Resolve a planned clothing layer's garment-shape masks into compositor
/// [`ShapeMask`]s (R14): each spec's static TGA (via `static_image`) plus its
/// wearable param weight (via `mask_weight`). A mask whose TGA is unavailable is
/// dropped rather than zeroing the layer.
fn resolve_masks(
    planned: &PlannedLayer,
    static_image: impl Fn(&str) -> Option<DecodedImage>,
    mask_weight: impl Fn(i32, WearableType) -> f32,
) -> Vec<ShapeMask> {
    planned
        .masks
        .iter()
        .filter_map(|spec| {
            static_image(spec.tga).map(|image| ShapeMask {
                image,
                weight: mask_weight(spec.param_id, planned.wearable),
                domain: spec.domain,
                multiply_blend: spec.multiply_blend,
            })
        })
        .collect()
}

/// The distinct static `character/` TGA file names the bake plans reference, so a
/// runtime can pre-load and decode them once for
/// [`region_layers`]'s `static_image` closure.
#[must_use]
pub fn static_layer_files() -> Vec<&'static str> {
    let mut files = Vec::new();
    for region in BakeRegion::ALL {
        for planned in region_plan(region) {
            if let LayerSource::Static(file) = planned.source
                && !files.contains(&file)
            {
                files.push(file);
            }
        }
    }
    files
}

/// The distinct garment-shape mask TGA file names the bake plans reference (R14),
/// so a runtime can pre-load and decode them once alongside
/// [`static_layer_files`] for [`region_layers`]'s `static_image` closure.
#[must_use]
pub fn shape_mask_files() -> Vec<&'static str> {
    let mut files = Vec::new();
    for region in BakeRegion::ALL {
        for planned in region_plan(region) {
            for spec in planned.masks {
                if !files.contains(&spec.tga) {
                    files.push(spec.tga);
                }
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::{Layer, LayerTint, region_layers, region_plan};
    use crate::composite::LayerKind;
    use crate::region::BakeRegion;
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_proto::WearableType;
    use sl_proto::avatar_texture as tex;
    use sl_texture::DecodedImage;

    /// A boxed error so a test can `?` on a missing layer without `expect`.
    type TestError = Box<dyn std::error::Error>;

    /// Assert an RGBA tint matches within a small tolerance.
    fn assert_tint(layer: &Layer, expected: [f32; 4]) {
        for (a, e) in layer.tint.iter().zip(expected.iter()) {
            assert!((a - e).abs() < 0.001, "{:?} != {expected:?}", layer.tint);
        }
    }

    /// A 2×2 solid RGBA image.
    fn image(rgba: [u8; 4]) -> DecodedImage {
        let mut pixels = Vec::new();
        for _ in 0..4 {
            pixels.extend_from_slice(&rgba);
        }
        DecodedImage {
            width: 2,
            height: 2,
            components: 4,
            discard_level: sl_proto::DiscardLevel::FULL,
            pixels: Bytes::from(pixels),
            aux: None,
        }
    }

    /// Every plan references non-baked layer slots that map to the plan's
    /// wearable type (the solid / static bases excepted), so the tables stay
    /// consistent with the `sl-proto` layer dictionary.
    #[test]
    fn plans_reference_consistent_slots() {
        for region in BakeRegion::ALL {
            for planned in region_plan(region) {
                if let super::LayerSource::Wearable(slot) = planned.source {
                    assert_eq!(
                        tex::layer_wearable_type(slot),
                        Some(planned.wearable),
                        "region {region:?} slot {slot}"
                    );
                }
            }
        }
    }

    /// Every static-TGA layer the plans reference is reported by
    /// [`static_layer_files`], with no duplicates.
    #[test]
    fn static_layer_files_are_listed_and_unique() {
        let files = super::static_layer_files();
        let mut seen = std::collections::HashSet::new();
        for file in &files {
            assert!(seen.insert(*file), "duplicate static file {file}");
        }
        for region in BakeRegion::ALL {
            for planned in region_plan(region) {
                if let super::LayerSource::Static(file) = planned.source {
                    assert!(files.contains(&file), "missing static file {file}");
                }
            }
        }
    }

    /// Each "universal" region composites a single universal-tattoo layer from a
    /// worn `WT_UNIVERSAL` wearable's matching slot (R22 parity), and nothing when
    /// none is worn — so an unworn universal bake is never published.
    #[test]
    fn universal_regions_composite_their_tattoo_slot() {
        let cases = [
            (BakeRegion::LeftArm, tex::LEFT_ARM_TATTOO),
            (BakeRegion::LeftLeg, tex::LEFT_LEG_TATTOO),
            (BakeRegion::Aux1, tex::AUX1_TATTOO),
            (BakeRegion::Aux2, tex::AUX2_TATTOO),
            (BakeRegion::Aux3, tex::AUX3_TATTOO),
        ];
        for (region, slot) in cases {
            assert_eq!(region_plan(region).len(), 1, "{} plan", region.name());
            // A worn universal wearable supplying the slot → one composited layer.
            let worn = region_layers(
                region,
                |wearable| wearable == WearableType::Universal,
                |s| (s == slot).then(|| image([1, 2, 3, 255])),
                |_file| None,
                |_tint, _wearable| [1.0, 1.0, 1.0, 1.0],
                |_id, _wearable| 0.0,
            );
            assert_eq!(worn.len(), 1, "{} worn", region.name());
            // Nothing worn → no layers → the region is skipped (no empty bake).
            let bare = region_layers(
                region,
                |_wearable| false,
                |_slot| None,
                |_file| None,
                |_tint, _wearable| [1.0, 1.0, 1.0, 1.0],
                |_id, _wearable| 0.0,
            );
            assert!(bare.is_empty(), "{} bare", region.name());
        }
    }

    /// A skin-only outfit yields the solid skin base plus the bodypaint layer on
    /// the head, and nothing for clothing slots.
    #[test]
    fn skin_only_head_has_base_and_bodypaint() -> Result<(), TestError> {
        let layers = region_layers(
            BakeRegion::Head,
            |wearable| wearable == WearableType::Skin,
            |slot| (slot == tex::HEAD_BODYPAINT).then(|| image([200, 150, 100, 255])),
            // No static TGAs available: the skin-grain base falls back to a solid
            // skin-tone fill, the face-colour detail is dropped.
            |_file| None,
            |tint, _wearable| match tint {
                LayerTint::Global("skin_color") => [0.8, 0.6, 0.5, 1.0],
                _ => [1.0, 1.0, 1.0, 1.0],
            },
            |_id, _wearable| 0.0,
        );
        // Solid skin base (no image) + head_bodypaint blend (image). No alpha /
        // tattoos worn.
        assert_eq!(layers.len(), 2);
        let base = layers.first().ok_or("base layer")?;
        assert_eq!(base.kind, LayerKind::Base);
        assert!(base.image.is_none());
        assert_tint(base, [0.8, 0.6, 0.5, 1.0]);
        let paint = layers.get(1).ok_or("bodypaint layer")?;
        assert_eq!(paint.kind, LayerKind::Blend);
        assert!(paint.image.is_some());
        Ok(())
    }

    /// The eyes bake gains a white sclera base from `eyewhite.tga` when the
    /// static TGA is available, with the iris blended over it.
    #[test]
    fn eyes_gain_white_sclera_base() -> Result<(), TestError> {
        let layers = region_layers(
            BakeRegion::Eyes,
            |wearable| wearable == WearableType::Eyes,
            |slot| (slot == tex::EYES_IRIS).then(|| image([90, 60, 30, 255])),
            // The eye sclera TGA is available (a solid white here).
            |file| (file == "eyewhite.tga").then(|| image([255, 255, 255, 255])),
            |_tint, _wearable| [1.0, 1.0, 1.0, 1.0],
            |_id, _wearable| 0.0,
        );
        // White sclera base (from the TGA) + the iris blended over it.
        assert_eq!(layers.len(), 2);
        let sclera = layers.first().ok_or("sclera base")?;
        assert_eq!(sclera.kind, LayerKind::Base);
        assert!(sclera.image.is_some());
        let iris = layers.get(1).ok_or("iris layer")?;
        assert_eq!(iris.kind, LayerKind::Blend);
        assert!(iris.image.is_some());
        Ok(())
    }

    /// A static base whose TGA is unavailable falls back to a solid tint fill, so
    /// the region never loses its opaque foundation.
    #[test]
    fn static_base_falls_back_to_solid() -> Result<(), TestError> {
        let layers = region_layers(
            BakeRegion::Eyes,
            |wearable| wearable == WearableType::Eyes,
            |_slot| None,
            // No static TGAs available.
            |_file| None,
            |_tint, _wearable| [0.5, 0.5, 0.5, 1.0],
            |_id, _wearable| 0.0,
        );
        // Just the fallback solid sclera base (iris has no texture).
        assert_eq!(layers.len(), 1);
        let base = layers.first().ok_or("fallback base")?;
        assert_eq!(base.kind, LayerKind::Base);
        assert!(base.image.is_none());
        Ok(())
    }

    /// With no skin worn and no textures, a bake region has no layers.
    #[test]
    fn nothing_worn_is_empty() {
        let layers = region_layers(
            BakeRegion::UpperBody,
            |_wearable| false,
            |_slot| None,
            |_file| None,
            |_tint, _wearable| [1.0, 1.0, 1.0, 1.0],
            |_id, _wearable| 0.0,
        );
        assert!(layers.is_empty());
    }

    /// A shirt over skin: base, bodypaint (if present), shirt, in that order; the
    /// shirt is tinted from its colour params.
    #[test]
    fn shirt_over_skin_orders_and_tints() -> Result<(), TestError> {
        let layers = region_layers(
            BakeRegion::UpperBody,
            |wearable| matches!(wearable, WearableType::Skin | WearableType::Shirt),
            |slot| (slot == tex::UPPER_SHIRT).then(|| image([10, 20, 200, 255])),
            |_file| None,
            |tint, _wearable| match tint {
                LayerTint::Global("skin_color") => [0.9, 0.7, 0.6, 1.0],
                LayerTint::Params(_) => [0.2, 0.2, 0.9, 1.0],
                _ => [1.0, 1.0, 1.0, 1.0],
            },
            |_id, _wearable| 0.0,
        );
        // Solid skin base + the shirt (no bodypaint / undershirt / etc. textures).
        assert_eq!(layers.len(), 2);
        let base = layers.first().ok_or("base layer")?;
        assert_eq!(base.kind, LayerKind::Base);
        assert!(base.image.is_none());
        let shirt = layers.get(1).ok_or("shirt layer")?;
        assert_eq!(shirt.kind, LayerKind::Blend);
        assert_tint(shirt, [0.2, 0.2, 0.9, 1.0]);
        Ok(())
    }

    /// A shirt garment layer carries its resolved garment-shape masks (R14) when
    /// the shape-mask TGAs are available, driven by the wearable's params; a
    /// non-garment layer (the skin base) carries none.
    #[test]
    fn garment_layer_resolves_shape_masks() -> Result<(), TestError> {
        let layers = region_layers(
            BakeRegion::UpperBody,
            |wearable| matches!(wearable, WearableType::Skin | WearableType::Shirt),
            |slot| (slot == tex::UPPER_SHIRT).then(|| image([10, 20, 200, 255])),
            // Every shape-mask TGA resolves to a stand-in image.
            |_file| Some(image([255, 255, 255, 255])),
            |_tint, _wearable| [1.0, 1.0, 1.0, 1.0],
            // Sleeve length 0.7 for the shirt's driving params.
            |_id, _wearable| 0.7,
        );
        // The only garment worn is the shirt, so it is the one layer with masks.
        let shirt = layers
            .iter()
            .find(|layer| !layer.masks.is_empty())
            .ok_or("shirt layer")?;
        assert_eq!(shirt.kind, LayerKind::Blend);
        // The shirt carries its four shape masks (sleeve, bottom, collar, back).
        assert_eq!(shirt.masks.len(), super::SHIRT_MASKS.len());
        let sleeve = shirt.masks.first().ok_or("sleeve mask")?;
        assert!((sleeve.weight - 0.7).abs() < 1.0e-6);
        assert!(!sleeve.multiply_blend);
        // The skin base is not a garment and carries no masks.
        let base = layers.first().ok_or("base layer")?;
        assert_eq!(base.kind, LayerKind::Base);
        assert!(base.masks.is_empty());
        Ok(())
    }

    /// `shape_mask_files` lists every referenced garment-shape mask TGA once.
    #[test]
    fn shape_mask_files_are_listed_and_unique() {
        let files = super::shape_mask_files();
        let mut seen = std::collections::HashSet::new();
        for file in &files {
            assert!(seen.insert(*file), "duplicate shape-mask file {file}");
        }
        for region in BakeRegion::ALL {
            for planned in region_plan(region) {
                for spec in planned.masks {
                    assert!(
                        files.contains(&spec.tga),
                        "missing shape-mask file {}",
                        spec.tga
                    );
                }
            }
        }
    }
}
