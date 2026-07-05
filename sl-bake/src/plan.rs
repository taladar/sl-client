//! The per-region **bake plan**: which worn-wearable texture layers feed each
//! base-body bake, in composite order, and how each is tinted (P15.2).
//!
//! The reference viewer's full `LLTexLayerSet` is far richer than the P15.1
//! compositor models — it also carries procedural cosmetic layers (skin shading,
//! lipstick, blush, freckles, bump maps) driven purely by visual params with no
//! wearable texture. Those need a per-param procedural renderer the simplified
//! [`composite`](crate::composite) engine does not have, so the plan here keeps
//! only the layers backed by a **worn wearable's texture** (the skin bodypaint,
//! clothing, tattoos, alpha masks) plus the solid skin-tone base — the inputs a
//! client-side bake actually composites from fetched assets. Layer order and
//! membership follow `avatar_lad.xml`'s `<layer_set>` blocks.
//!
//! [`region_layers`] turns a plan into the ordered [`Layer`] list the compositor
//! wants, given closures that resolve each slot's decoded texture and each
//! layer's tint from the worn wearable assets (the runtime crates own the fetch /
//! decode / param lookup; this stays I/O-free).

use crate::composite::{Layer, LayerKind, TexGen};
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

/// One planned layer of a bake region: which worn wearable and `TextureEntry`
/// slot supply it, how it composites, and how it is tinted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannedLayer {
    /// The avatar `TextureEntry` layer slot supplying this layer's texture, or
    /// `None` for a solid tint fill (the skin-tone base).
    pub slot: Option<usize>,
    /// The wearable type that supplies this layer (decides whether it is worn and
    /// which wearable's params tint it).
    pub wearable: WearableType,
    /// How this layer composites over the ones below it.
    pub kind: LayerKind,
    /// How the layer texture maps onto the body.
    pub tex_gen: TexGen,
    /// Where this layer's tint comes from.
    pub tint: LayerTint,
}

impl PlannedLayer {
    /// A solid tint-fill base layer (no texture) from `wearable`, tinted by
    /// `tint` — the skin-tone foundation of the head / upper / lower bakes.
    const fn solid_base(wearable: WearableType, tint: LayerTint) -> Self {
        Self {
            slot: None,
            wearable,
            kind: LayerKind::Base,
            tex_gen: TexGen::Default,
            tint,
        }
    }

    /// A textured layer of `kind` from `wearable`'s `slot`, tinted by `tint`.
    const fn textured(
        slot: usize,
        wearable: WearableType,
        kind: LayerKind,
        tint: LayerTint,
    ) -> Self {
        Self {
            slot: Some(slot),
            wearable,
            kind,
            tex_gen: TexGen::Default,
            tint,
        }
    }
}

/// Shorthands for the three composite kinds, keeping the tables compact.
use LayerKind::{AlphaMask, Base, Blend};
use WearableType::{
    Alpha, Eyes, Gloves, Hair, Jacket, Pants, Shirt, Shoes, Skin, Skirt, Socks, Tattoo, Underpants,
    Undershirt, Universal,
};

/// The head bake's layers, in composite order (skin tone → skin paint → alpha →
/// tattoos), following `avatar_lad.xml`'s `head` `<layer_set>`.
const HEAD: &[PlannedLayer] = &[
    PlannedLayer::solid_base(Skin, LayerTint::Global("skin_color")),
    PlannedLayer::textured(tex::HEAD_BODYPAINT, Skin, Blend, LayerTint::White),
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
    PlannedLayer::solid_base(Skin, LayerTint::Global("skin_color")),
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
    PlannedLayer::textured(
        tex::UPPER_UNDERSHIRT,
        Undershirt,
        Blend,
        LayerTint::Params(&[821, 822, 823]),
    ),
    PlannedLayer::textured(
        tex::UPPER_GLOVES,
        Gloves,
        Blend,
        LayerTint::Params(&[827, 829, 830]),
    ),
    PlannedLayer::textured(
        tex::UPPER_SHIRT,
        Shirt,
        Blend,
        LayerTint::Params(&[803, 804, 805]),
    ),
    PlannedLayer::textured(
        tex::UPPER_JACKET,
        Jacket,
        Blend,
        LayerTint::Params(&[831, 832, 833]),
    ),
    PlannedLayer::textured(tex::UPPER_ALPHA, Alpha, AlphaMask, LayerTint::White),
];

/// The lower-body bake's layers, in composite order.
const LOWER: &[PlannedLayer] = &[
    PlannedLayer::solid_base(Skin, LayerTint::Global("skin_color")),
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
    PlannedLayer::textured(
        tex::LOWER_UNDERPANTS,
        Underpants,
        Blend,
        LayerTint::Params(&[824, 825, 826]),
    ),
    PlannedLayer::textured(
        tex::LOWER_SOCKS,
        Socks,
        Blend,
        LayerTint::Params(&[818, 819, 820]),
    ),
    PlannedLayer::textured(
        tex::LOWER_SHOES,
        Shoes,
        Blend,
        LayerTint::Params(&[812, 813, 817]),
    ),
    PlannedLayer::textured(
        tex::LOWER_PANTS,
        Pants,
        Blend,
        LayerTint::Params(&[806, 807, 808]),
    ),
    PlannedLayer::textured(
        tex::LOWER_JACKET,
        Jacket,
        Blend,
        LayerTint::Params(&[809, 810, 811]),
    ),
    PlannedLayer::textured(tex::LOWER_ALPHA, Alpha, AlphaMask, LayerTint::White),
];

/// The eyes bake's layers: the iris (tinted by eye colour), its alpha, and a
/// universal tattoo overlay.
const EYES: &[PlannedLayer] = &[
    PlannedLayer::textured(tex::EYES_IRIS, Eyes, Base, LayerTint::Global("eye_color")),
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
    }
}

/// Assemble the ordered [`Layer`] list a bake region composites from, resolving
/// each planned layer against the worn wearables.
///
/// - `worn(type)` — whether a wearable of that type is worn (gates the solid
///   skin-tone base, which has no texture to key off).
/// - `image_for(slot)` — the decoded texture for a `TextureEntry` layer slot, or
///   `None` when no worn wearable supplies it (that layer is then omitted).
/// - `tint_for(tint, wearable)` — the linear-RGBA tint for a layer, resolved from
///   the wearable's visual params (opaque white for [`LayerTint::White`]).
///
/// A textured layer whose `image_for` yields `None` is skipped; the solid base is
/// skipped unless its wearable is worn. The result feeds
/// [`composite_region`](crate::composite_region).
pub fn region_layers(
    region: BakeRegion,
    worn: impl Fn(WearableType) -> bool,
    image_for: impl Fn(usize) -> Option<DecodedImage>,
    tint_for: impl Fn(LayerTint, WearableType) -> [f32; 4],
) -> Vec<Layer> {
    let mut layers = Vec::new();
    for planned in region_plan(region) {
        let tint = tint_for(planned.tint, planned.wearable);
        let layer = match planned.slot {
            None => {
                if !worn(planned.wearable) {
                    continue;
                }
                Layer::solid(planned.kind, tint).with_tex_gen(planned.tex_gen)
            }
            Some(slot) => {
                let Some(image) = image_for(slot) else {
                    continue;
                };
                let base = match planned.kind {
                    LayerKind::Base => Layer::base(image),
                    LayerKind::Blend => Layer::blend(image),
                    LayerKind::AlphaMask => Layer::alpha_mask(image),
                };
                base.with_tint(tint).with_tex_gen(planned.tex_gen)
            }
        };
        layers.push(layer);
    }
    layers
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
        }
    }

    /// Every plan references non-baked layer slots that map to the plan's
    /// wearable type (the solid base excepted), so the tables stay consistent
    /// with the `sl-proto` layer dictionary.
    #[test]
    fn plans_reference_consistent_slots() {
        for region in BakeRegion::ALL {
            for planned in region_plan(region) {
                if let Some(slot) = planned.slot {
                    assert_eq!(
                        tex::layer_wearable_type(slot),
                        Some(planned.wearable),
                        "region {region:?} slot {slot}"
                    );
                }
            }
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
            |tint, _wearable| match tint {
                LayerTint::Global("skin_color") => [0.8, 0.6, 0.5, 1.0],
                _ => [1.0, 1.0, 1.0, 1.0],
            },
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

    /// With no skin worn and no textures, a bake region has no layers.
    #[test]
    fn nothing_worn_is_empty() {
        let layers = region_layers(
            BakeRegion::UpperBody,
            |_wearable| false,
            |_slot| None,
            |_tint, _wearable| [1.0, 1.0, 1.0, 1.0],
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
            |tint, _wearable| match tint {
                LayerTint::Global("skin_color") => [0.9, 0.7, 0.6, 1.0],
                LayerTint::Params(_) => [0.2, 0.2, 0.9, 1.0],
                _ => [1.0, 1.0, 1.0, 1.0],
            },
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
}
