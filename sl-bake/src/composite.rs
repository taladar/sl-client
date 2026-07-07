//! Compositing an ordered stack of wearable layers into one baked region RGBA.
//!
//! [`composite_region`] walks a region's [`Layer`] stack bottom-to-top over a
//! transparent canvas, following the reference viewer's `LLTexLayerSet` render:
//! the [`LayerKind::Base`] skin writes all four channels, [`LayerKind::Blend`]
//! layers are alpha-composited *over* what is below, and [`LayerKind::AlphaMask`]
//! layers carve the destination alpha so the underlying body shows through as
//! transparent. Each layer texture is bilinearly resampled to the bake
//! resolution, so the input images need not share a size.

use crate::region::BakeRegion;
use sl_proto::DiscardLevel;
use sl_texture::DecodedImage;

/// The number of channels in a canonical RGBA8 pixel (matching
/// [`DecodedImage::pixels`]).
const RGBA_CHANNELS: usize = 4;

/// The alpha (opacity) channel offset within an RGBA8 pixel.
const ALPHA_OFFSET: usize = 3;

/// How a wearable layer's texture maps onto the avatar body — the reference
/// viewer's `tex_gen`. Retained as metadata on each [`Layer`]: at the raster
/// composite level every layer is already expressed in the bake's own UV frame,
/// so both modes are resampled identically here; the distinction matters when
/// *sourcing* a layer texture (P15.2), which this crate does not do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TexGen {
    /// `TEX_GEN_DEFAULT`: the texture follows the body mesh's own UVs.
    #[default]
    Default,
    /// `TEX_GEN_PLANAR`: the texture is planar-projected (e.g. some skin
    /// layers).
    Planar,
}

/// How a [`Layer`] combines with the region canvas built up from the layers
/// below it, mirroring the reference viewer's per-layer blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    /// The opaque base (the skin): writes all four RGBA channels, replacing
    /// whatever is below. The reference viewer's `mWriteAllChannels`.
    Base,
    /// A standard source-over alpha blend (tattoo, clothing): composited *over*
    /// the canvas, modulated by the layer's tint and opacity.
    Blend,
    /// An alpha mask (an alpha wearable): leaves colour untouched and carves the
    /// destination alpha channel, hiding the body where the mask is opaque.
    AlphaMask,
}

/// A garment-shape alpha mask bounding a clothing [`Layer`] to its garment
/// extent (R14): a static grayscale mask image plus the wearable param weight and
/// processing that turn it into a per-texel coverage alpha, exactly as the
/// reference viewer's `LLTexLayerParamAlpha` (`avatar_lad.xml`'s `<param_alpha>`)
/// does.
///
/// A clothing layer's `local_texture` covers the *whole* body-region UV, so a
/// solid-fabric shirt or pants would otherwise paint the bare hands and feet too.
/// The reference viewer instead multiplies each garment layer's coverage by a
/// stack of these masks — sleeve length, shirt bottom, collar, pants length,
/// waist, … — driven by the wearable's shape params, so the fabric is bounded to
/// the garment (hands and feet stay skin). See [`Layer::masks`].
#[derive(Debug, Clone)]
pub struct ShapeMask {
    /// The static grayscale mask image; its red channel is the raw mask value the
    /// LUT below processes (the reference viewer's one-component alpha TGA).
    pub image: DecodedImage,
    /// The wearable's weight for this mask's driving param (the raw param value,
    /// e.g. sleeve length `0.7`), applied through the `domain` ramp.
    pub weight: f32,
    /// The width of the input→output ramp in the mask LUT; `0` is a hard
    /// threshold. The reference viewer's `param_alpha` `domain`.
    pub domain: f32,
    /// Whether this mask *multiplies* into the accumulated coverage (approximating
    /// a `min()`), or *adds* to it (approximating a `max()`). The reference
    /// viewer's per-`param_alpha` `multiply_blend`.
    pub multiply_blend: bool,
}

/// One wearable layer feeding a bake region: an optional decoded texture plus
/// the parameters that govern how it composites.
///
/// A layer with no [`Layer::image`] is a solid fill of its [`Layer::tint`] (the
/// reference viewer's colour-only layer, e.g. a plain skin tone). Construct with
/// [`Layer::base`] / [`Layer::blend`] / [`Layer::alpha_mask`] and adjust with the
/// `with_*` builders.
#[derive(Debug, Clone)]
pub struct Layer {
    /// The decoded layer texture, or `None` for a solid [`Layer::tint`] fill.
    pub image: Option<DecodedImage>,
    /// How this layer combines with the layers below it.
    pub kind: LayerKind,
    /// A linear RGBA multiply applied to the layer's texels; the alpha component
    /// is the layer's overall opacity (the reference viewer's `net_color`). Each
    /// channel is `0.0..=1.0`.
    pub tint: [f32; 4],
    /// How the layer texture maps onto the body (metadata; see [`TexGen`]).
    pub tex_gen: TexGen,
    /// For an [`LayerKind::AlphaMask`] layer, invert the mask so it carves where
    /// the mask is *transparent* instead of opaque (the reference viewer's
    /// per-layer `invert` flag). Ignored for other kinds.
    pub invert_mask: bool,
    /// The garment-shape masks bounding this (clothing) layer to its garment
    /// extent (R14). Empty for a non-garment layer (skin base, tattoo, …), which
    /// then covers the whole region. Applied only to a [`LayerKind::Blend`] layer.
    pub masks: Vec<ShapeMask>,
}

/// Opaque white, the identity tint (no colour change, full opacity).
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

impl Layer {
    /// An opaque base (skin) layer from `image`, tinted white — the region's
    /// foundation. See [`LayerKind::Base`].
    #[must_use]
    pub const fn base(image: DecodedImage) -> Self {
        Self {
            image: Some(image),
            kind: LayerKind::Base,
            tint: WHITE,
            tex_gen: TexGen::Default,
            invert_mask: false,
            masks: Vec::new(),
        }
    }

    /// A source-over blend (tattoo / clothing) layer from `image`, tinted white.
    /// See [`LayerKind::Blend`].
    #[must_use]
    pub const fn blend(image: DecodedImage) -> Self {
        Self {
            image: Some(image),
            kind: LayerKind::Blend,
            tint: WHITE,
            tex_gen: TexGen::Default,
            invert_mask: false,
            masks: Vec::new(),
        }
    }

    /// An alpha-mask (alpha wearable) layer from `image` that carves the
    /// destination alpha where the mask is opaque. See [`LayerKind::AlphaMask`].
    #[must_use]
    pub const fn alpha_mask(image: DecodedImage) -> Self {
        Self {
            image: Some(image),
            kind: LayerKind::AlphaMask,
            tint: WHITE,
            tex_gen: TexGen::Default,
            invert_mask: false,
            masks: Vec::new(),
        }
    }

    /// A solid-colour layer of `kind` with no texture — the canvas is filled
    /// with `tint` (for [`LayerKind::AlphaMask`], `tint`'s alpha is the mask
    /// value everywhere).
    #[must_use]
    pub const fn solid(kind: LayerKind, tint: [f32; 4]) -> Self {
        Self {
            image: None,
            kind,
            tint,
            tex_gen: TexGen::Default,
            invert_mask: false,
            masks: Vec::new(),
        }
    }

    /// Set the layer's RGBA tint (its alpha is the layer opacity).
    #[must_use]
    pub const fn with_tint(mut self, tint: [f32; 4]) -> Self {
        self.tint = tint;
        self
    }

    /// Set the layer's overall opacity (the tint's alpha channel).
    #[must_use]
    pub const fn with_opacity(mut self, opacity: f32) -> Self {
        self.tint[ALPHA_OFFSET] = opacity;
        self
    }

    /// Set the layer's [`TexGen`] mapping mode.
    #[must_use]
    pub const fn with_tex_gen(mut self, tex_gen: TexGen) -> Self {
        self.tex_gen = tex_gen;
        self
    }

    /// Invert an [`LayerKind::AlphaMask`] layer (carve where the mask is
    /// transparent instead of opaque).
    #[must_use]
    pub const fn inverted(mut self) -> Self {
        self.invert_mask = true;
        self
    }

    /// Bound this (clothing) layer to its garment extent with `masks` (R14): the
    /// stack of [`ShapeMask`]s whose combined coverage modulates the layer's
    /// per-texel alpha, keeping a solid-fabric garment off the bare hands / feet.
    #[must_use]
    pub fn with_masks(mut self, masks: Vec<ShapeMask>) -> Self {
        self.masks = masks;
        self
    }
}

/// A composited bake region: tightly packed 8-bit RGBA pixels for one square
/// region, ready to drape as a material or J2C-encode for upload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BakedImage {
    /// The region this bake covers.
    pub region: BakeRegion,
    /// The bake's side length in pixels (bakes are square).
    pub size: u32,
    /// Tightly packed 8-bit RGBA pixels, `size * size * 4` bytes, row-major.
    pub pixels: Vec<u8>,
}

impl BakedImage {
    /// View the bake as a full-resolution [`DecodedImage`] (RGBA8,
    /// [`DiscardLevel::FULL`]) so it can flow through the same
    /// texture-consuming paths as a fetched-and-decoded avatar bake.
    #[must_use]
    pub fn to_decoded_image(&self) -> DecodedImage {
        DecodedImage {
            width: self.size,
            height: self.size,
            components: RGBA_CHANNELS_U16,
            discard_level: DiscardLevel::FULL,
            pixels: bytes::Bytes::copy_from_slice(&self.pixels),
        }
    }
}

/// [`RGBA_CHANNELS`] as the `u16` [`DecodedImage::components`] wants.
const RGBA_CHANNELS_U16: u16 = 4;

/// Composite `region`'s ordered `layers` (bottom-to-top) into a `size`×`size`
/// baked RGBA image.
///
/// The canvas starts fully transparent; each layer is applied per
/// [`LayerKind`]. A `size` of `0` yields an empty bake. Layer textures are
/// bilinearly resampled to the canvas, so they need not match `size` or each
/// other.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `composite_region` reads clearly"
)]
pub fn composite_region(region: BakeRegion, size: u32, layers: &[Layer]) -> BakedImage {
    let side = usize_from_u32(size);
    let pixel_count = side.saturating_mul(side);
    // A working canvas of linear RGBA in `0.0..=1.0`, transparent to start.
    let mut canvas = vec![[0.0_f32; 4]; pixel_count];

    for layer in layers {
        apply_layer(&mut canvas, side, layer);
    }

    let mut pixels = Vec::with_capacity(pixel_count.saturating_mul(RGBA_CHANNELS));
    for texel in &canvas {
        for channel in texel {
            pixels.push(u8_from_unit_f32(*channel));
        }
    }

    BakedImage {
        region,
        size,
        pixels,
    }
}

/// A [`ShapeMask`] readied for per-texel sampling: its grayscale sampler plus the
/// LUT parameters that turn a raw mask value into a coverage alpha.
struct MaskSampler<'pixels> {
    /// Bilinear sampler over the mask's grayscale image.
    sampler: LayerSampler<'pixels>,
    /// The wearable param weight applied through the LUT.
    weight: f32,
    /// The LUT ramp width (`0` is a hard threshold).
    domain: f32,
    /// Whether this mask multiplies (min) or adds (max) into the coverage.
    multiply_blend: bool,
}

/// Apply one `layer` to the working `canvas` (`side`×`side` linear RGBA).
fn apply_layer(canvas: &mut [[f32; 4]], side: usize, layer: &Layer) {
    let sampler = layer.image.as_ref().and_then(LayerSampler::new);
    // Whether the source carries a real alpha channel; a solid fill (no image)
    // is treated as authoritative (its opacity is the tint alpha).
    let has_alpha = sampler.as_ref().is_none_or(|sampler| sampler.has_alpha);
    // Ready the garment-shape masks (R14) for per-texel coverage; a mask whose
    // image is degenerate is dropped rather than zeroing the whole layer.
    let masks: Vec<MaskSampler<'_>> = layer
        .masks
        .iter()
        .filter_map(|mask| {
            LayerSampler::new(&mask.image).map(|sampler| MaskSampler {
                sampler,
                weight: mask.weight,
                domain: mask.domain,
                multiply_blend: mask.multiply_blend,
            })
        })
        .collect();
    for y in 0..side {
        for x in 0..side {
            let index = y.saturating_mul(side).saturating_add(x);
            let Some(dst) = canvas.get_mut(index) else {
                continue;
            };
            // Centre-sample the layer texture (or a flat white for a solid
            // fill) at this canvas pixel, in normalised UV.
            let (u, v) = pixel_uv(x, y, side);
            let source = match &sampler {
                Some(sampler) => sampler.sample(u, v),
                None => WHITE,
            };
            let coverage = mask_coverage(&masks, u, v);
            blend_pixel(dst, layer, source, has_alpha, coverage);
        }
    }
}

/// The combined garment-shape coverage of `masks` at normalised UV `(u, v)`, in
/// `0.0..=1.0` — `1.0` (no bounding) when there are no masks. Mirrors the
/// reference viewer's `LLTexLayer::renderMorphMasks` accumulation: the first mask
/// seeds the coverage (a `multiply_blend` mask multiplies against a full `1.0`
/// buffer, an additive one starts from `0.0`), and each later mask multiplies
/// (min) or adds (max) into it.
fn mask_coverage(masks: &[MaskSampler<'_>], u: f32, v: f32) -> f32 {
    let Some(first) = masks.first() else {
        return 1.0;
    };
    let mut coverage = if first.multiply_blend { 1.0 } else { 0.0 };
    for mask in masks {
        // The mask is a one-component (grayscale) image; its red channel is the
        // raw value the LUT processes.
        let raw = mask.sampler.sample(u, v)[0];
        let value = process_mask_alpha(raw, mask.weight, mask.domain);
        if mask.multiply_blend {
            coverage *= value;
        } else {
            coverage = (coverage + value).min(1.0);
        }
    }
    coverage.clamp(0.0, 1.0)
}

/// Process a raw grayscale mask value (`0.0..=1.0`) into a coverage alpha through
/// the reference viewer's `LLImageTGA::decodeAndProcess` LUT: a `domain`-wide
/// input→output ramp offset by the param `weight` (`domain == 0` is a hard
/// threshold at `1 - weight`).
fn process_mask_alpha(raw: f32, weight: f32, domain: f32) -> f32 {
    let inv_weight = (1.0 - weight).clamp(0.0, 1.0);
    if domain > 0.0 {
        let scale = 1.0 / domain;
        let offset = (1.0 - domain) * inv_weight;
        let bias = -(scale * offset);
        (raw * scale + bias).clamp(0.0, 1.0)
    } else if raw >= inv_weight {
        1.0
    } else {
        0.0
    }
}

/// The normalised centre-sampled UV of canvas pixel `(x, y)` in a `side`×`side`
/// canvas. A degenerate (zero) side maps everything to the origin.
fn pixel_uv(x: usize, y: usize, side: usize) -> (f32, f32) {
    if side == 0 {
        return (0.0, 0.0);
    }
    let denom = f32_from_usize(side);
    let u = (f32_from_usize(x) + 0.5) / denom;
    let v = (f32_from_usize(y) + 0.5) / denom;
    (u, v)
}

/// Combine one already-sampled `source` RGBA (the raw layer texel) into the
/// destination `dst` per the layer's [`LayerKind`] and tint. `source_has_alpha`
/// says whether the source carried a real alpha channel, which decides how an
/// [`LayerKind::AlphaMask`] reads its mask value.
fn blend_pixel(
    dst: &mut [f32; 4],
    layer: &Layer,
    source: [f32; 4],
    source_has_alpha: bool,
    coverage: f32,
) {
    let tint = layer.tint;
    match layer.kind {
        LayerKind::Base => {
            // Replace all channels with the modulated texel (opaque skin base).
            *dst = [
                source[0] * tint[0],
                source[1] * tint[1],
                source[2] * tint[2],
                source[3] * tint[3],
            ];
        }
        LayerKind::Blend => {
            // Standard (non-premultiplied) source-over composite, bounded by the
            // garment-shape mask coverage (R14) so a clothing layer paints only
            // its garment extent, not the bare hands / feet.
            let src_a = (source[3] * tint[3] * coverage).clamp(0.0, 1.0);
            let inv = 1.0 - src_a;
            let over = |src: f32, dst: f32| src * src_a + dst * inv;
            *dst = [
                over(source[0] * tint[0], dst[0]),
                over(source[1] * tint[1], dst[1]),
                over(source[2] * tint[2], dst[2]),
                src_a + dst[ALPHA_OFFSET] * inv,
            ];
        }
        LayerKind::AlphaMask => {
            // Carve the destination alpha; colour is untouched. The mask value
            // is the source's opacity: its alpha channel, or — for a source
            // stored without alpha (a grey mask) — its luminance, mirroring the
            // reference viewer's mask-from-grey. Optionally inverted, then scaled
            // by the layer opacity.
            let raw = if source_has_alpha {
                source[ALPHA_OFFSET]
            } else {
                luminance(source)
            };
            let mut mask = raw.clamp(0.0, 1.0);
            if layer.invert_mask {
                mask = 1.0 - mask;
            }
            let opacity = tint[ALPHA_OFFSET].clamp(0.0, 1.0);
            // opacity 1 → keep = mask; opacity 0 → keep everything.
            let keep = 1.0 - opacity * (1.0 - mask);
            dst[ALPHA_OFFSET] *= keep;
        }
    }
}

/// A borrowed view over a decoded layer texture offering bilinear RGBA sampling
/// in normalised UV, canonicalising a no-alpha source's opacity to its
/// luminance (so an alpha mask stored as a grey texture still carves).
struct LayerSampler<'pixels> {
    /// The texture width in pixels.
    width: usize,
    /// The texture height in pixels.
    height: usize,
    /// Whether the source carried a real alpha channel; when `false` the alpha
    /// the caller sees is the pixel's luminance rather than the decoder-filled
    /// opaque 255.
    has_alpha: bool,
    /// The tightly packed RGBA8 pixels, row-major (`(y * width + x) * 4`).
    pixels: &'pixels [u8],
}

impl<'pixels> LayerSampler<'pixels> {
    /// View `image` as a sampler, or `None` when it is degenerate (zero
    /// dimension or too few pixel bytes for its geometry).
    fn new(image: &'pixels DecodedImage) -> Option<Self> {
        let width = usize::try_from(image.width).ok()?;
        let height = usize::try_from(image.height).ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        let needed = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(RGBA_CHANNELS))?;
        if image.pixels.len() < needed {
            return None;
        }
        Some(Self {
            width,
            height,
            // `components` is source-only metadata; a source with < 4 components
            // had its alpha filled opaque by the decoder, so we derive opacity
            // from luminance instead (the reference viewer's mask-from-grey).
            has_alpha: image.components >= RGBA_CHANNELS_U16,
            pixels: &image.pixels,
        })
    }

    /// Bilinearly sample the raw RGBA (`0.0..=1.0`) at normalised `(u, v)`.
    /// Whether the alpha channel is meaningful is [`Self::has_alpha`]; the
    /// caller substitutes luminance for a no-alpha mask source.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let max_x = self.width.saturating_sub(1);
        let max_y = self.height.saturating_sub(1);
        let fx = u.clamp(0.0, 1.0) * f32_from_usize(max_x);
        let fy = v.clamp(0.0, 1.0) * f32_from_usize(max_y);
        let x0 = usize_from_f32_floor(fx).min(max_x);
        let y0 = usize_from_f32_floor(fy).min(max_y);
        let x1 = x0.saturating_add(1).min(max_x);
        let y1 = y0.saturating_add(1).min(max_y);
        let tx = fx - f32_from_usize(x0);
        let ty = fy - f32_from_usize(y0);

        let top = lerp4(self.texel(x0, y0), self.texel(x1, y0), tx);
        let bottom = lerp4(self.texel(x0, y1), self.texel(x1, y1), tx);
        lerp4(top, bottom, ty)
    }

    /// The raw RGBA (`0.0..=1.0`) of the pixel at integer `(x, y)`.
    fn texel(&self, x: usize, y: usize) -> [f32; 4] {
        let base = y
            .saturating_mul(self.width)
            .saturating_add(x)
            .saturating_mul(RGBA_CHANNELS);
        let channel = |offset: usize| {
            let raw = base
                .checked_add(offset)
                .and_then(|index| self.pixels.get(index))
                .copied()
                .unwrap_or(0);
            f32::from(raw) * INV_U8_MAX
        };
        [channel(0), channel(1), channel(2), channel(ALPHA_OFFSET)]
    }
}

/// Rec. 601 luma of a linear RGBA source's colour, the reference viewer's
/// grey-mask reading for an alpha wearable stored without an alpha channel.
fn luminance(rgba: [f32; 4]) -> f32 {
    rgba[0] * 0.299 + rgba[1] * 0.587 + rgba[2] * 0.114
}

/// Reciprocal of `u8::MAX` as `f32`, to normalise a byte to `0.0..=1.0`.
const INV_U8_MAX: f32 = 1.0 / 255.0;

/// Linearly interpolate two RGBA quads.
fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Widen a small `usize` count to `f32`; pixel counts are far below the 24-bit
/// exact-integer range, so no precision is lost.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "pixel counts are small, well within f32's exact-integer range"
)]
const fn f32_from_usize(value: usize) -> f32 {
    value as f32
}

/// Floor a non-negative `f32` to `usize`; a negative or non-finite value (which
/// the clamped sampling coordinates cannot produce) maps to `0`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is a clamped, non-negative pixel coordinate; its floor fits usize"
)]
fn usize_from_f32_floor(value: f32) -> usize {
    if value.is_finite() && value >= 0.0 {
        value.floor() as usize
    } else {
        0
    }
}

/// Widen a `u32` to `usize` (lossless on every supported target).
fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// Quantise a linear `0.0..=1.0` channel to an 8-bit value, rounding to nearest;
/// out-of-range inputs are clamped.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0.0..=255.0 before truncation, so it fits u8"
)]
fn u8_from_unit_f32(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::{
        BakedImage, Layer, LayerKind, ShapeMask, TexGen, composite_region, process_mask_alpha,
    };
    use crate::region::BakeRegion;
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_texture::DecodedImage;

    /// A solid `size`×`size` image of one RGBA colour, with `components`
    /// recorded as source metadata (pixels are always RGBA8).
    fn solid_image(size: u32, rgba: [u8; 4], components: u16) -> DecodedImage {
        let side = usize::try_from(size).unwrap_or(0);
        let count = side.saturating_mul(side);
        let mut pixels = Vec::with_capacity(count.saturating_mul(4));
        for _ in 0..count {
            pixels.extend_from_slice(&rgba);
        }
        DecodedImage {
            width: size,
            height: size,
            components,
            discard_level: sl_proto::DiscardLevel::FULL,
            pixels: Bytes::from(pixels),
        }
    }

    /// The RGBA of the centre pixel of a bake.
    fn centre_pixel(bake: &BakedImage) -> [u8; 4] {
        let side = usize::try_from(bake.size).unwrap_or(0);
        let half = side.checked_div(2).unwrap_or(0);
        let mid = half.saturating_mul(side).saturating_add(half);
        let base = mid.saturating_mul(4);
        let at = |offset: usize| {
            bake.pixels
                .get(base.saturating_add(offset))
                .copied()
                .unwrap_or(0)
        };
        [at(0), at(1), at(2), at(3)]
    }

    #[test]
    fn empty_stack_is_transparent() {
        let bake = composite_region(BakeRegion::Head, 4, &[]);
        assert_eq!(bake.size, 4);
        assert_eq!(bake.pixels.len(), 4 * 4 * 4);
        assert!(bake.pixels.iter().all(|&b| b == 0));
    }

    #[test]
    fn zero_size_is_empty() {
        let bake = composite_region(
            BakeRegion::Head,
            0,
            &[Layer::base(solid_image(2, [10, 20, 30, 255], 3))],
        );
        assert_eq!(bake.size, 0);
        assert!(bake.pixels.is_empty());
    }

    #[test]
    fn base_layer_fills_opaque() {
        let skin = solid_image(8, [200, 150, 100, 255], 3);
        let bake = composite_region(BakeRegion::UpperBody, 8, &[Layer::base(skin)]);
        assert_eq!(centre_pixel(&bake), [200, 150, 100, 255]);
    }

    #[test]
    fn base_tint_multiplies() {
        // A white skin tinted half-red keeps red, halves green/blue.
        let skin = solid_image(8, [255, 255, 255, 255], 3);
        let layer = Layer::base(skin).with_tint([1.0, 0.5, 0.5, 1.0]);
        let bake = composite_region(BakeRegion::UpperBody, 8, &[layer]);
        assert_eq!(centre_pixel(&bake), [255, 128, 128, 255]);
    }

    #[test]
    fn blend_over_opaque_replaces() {
        // An opaque red base with an opaque blue blend on top → blue.
        let base = Layer::base(solid_image(8, [255, 0, 0, 255], 3));
        let over = Layer::blend(solid_image(8, [0, 0, 255, 255], 3));
        let bake = composite_region(BakeRegion::UpperBody, 8, &[base, over]);
        assert_eq!(centre_pixel(&bake), [0, 0, 255, 255]);
    }

    #[test]
    fn blend_half_opacity_mixes() {
        // Red base, blue blend at 50% opacity → halfway (128) each.
        let base = Layer::base(solid_image(8, [255, 0, 0, 255], 3));
        let over = Layer::blend(solid_image(8, [0, 0, 255, 255], 3)).with_opacity(0.5);
        let bake = composite_region(BakeRegion::UpperBody, 8, &[base, over]);
        let px = centre_pixel(&bake);
        // 255*0.5 = 128 (rounded) red kept, 255*0.5 blue added.
        assert_eq!(px, [128, 0, 128, 255]);
    }

    #[test]
    fn alpha_mask_carves_alpha_keeps_colour() {
        // Opaque skin, then a fully-transparent-source alpha mask (grey 0 → mask
        // value 0 → hide). Colour retained, alpha carved to 0.
        let base = Layer::base(solid_image(8, [200, 150, 100, 255], 3));
        // A grey source at 0 has luminance 0; components 3 → luminance mask.
        let mask = Layer::alpha_mask(solid_image(8, [0, 0, 0, 255], 3));
        let bake = composite_region(BakeRegion::LowerBody, 8, &[base, mask]);
        assert_eq!(centre_pixel(&bake), [200, 150, 100, 0]);
    }

    #[test]
    fn alpha_mask_white_keeps_everything() {
        // A white (luminance 1) mask keeps all alpha.
        let base = Layer::base(solid_image(8, [200, 150, 100, 255], 3));
        let mask = Layer::alpha_mask(solid_image(8, [255, 255, 255, 255], 3));
        let bake = composite_region(BakeRegion::LowerBody, 8, &[base, mask]);
        assert_eq!(centre_pixel(&bake), [200, 150, 100, 255]);
    }

    #[test]
    fn alpha_mask_inverted_flips() {
        // An inverted black mask (mask 0 → 1) keeps alpha.
        let base = Layer::base(solid_image(8, [200, 150, 100, 255], 3));
        let mask = Layer::alpha_mask(solid_image(8, [0, 0, 0, 255], 3)).inverted();
        let bake = composite_region(BakeRegion::LowerBody, 8, &[base, mask]);
        assert_eq!(centre_pixel(&bake), [200, 150, 100, 255]);
    }

    #[test]
    fn alpha_mask_uses_source_alpha_when_present() {
        // A 4-component source: alpha 0 hides even though RGB is white.
        let base = Layer::base(solid_image(8, [200, 150, 100, 255], 3));
        let mask = Layer::alpha_mask(solid_image(8, [255, 255, 255, 0], 4));
        let bake = composite_region(BakeRegion::LowerBody, 8, &[base, mask]);
        assert_eq!(centre_pixel(&bake), [200, 150, 100, 0]);
    }

    #[test]
    fn solid_layer_needs_no_image() {
        let base = Layer::solid(LayerKind::Base, [0.4, 0.6, 0.8, 1.0]);
        let bake = composite_region(BakeRegion::Eyes, 4, &[base]);
        // 0.4*255≈102, 0.6*255≈153, 0.8*255≈204.
        assert_eq!(centre_pixel(&bake), [102, 153, 204, 255]);
    }

    #[test]
    fn resamples_mismatched_layer_size() {
        // A 2×2 base and a 4×4 bake: the base still fills the whole region.
        let base = Layer::base(solid_image(2, [50, 60, 70, 255], 3));
        let bake = composite_region(BakeRegion::Head, 4, &[base]);
        assert_eq!(centre_pixel(&bake), [50, 60, 70, 255]);
    }

    #[test]
    fn to_decoded_image_round_trips_pixels() {
        let base = Layer::base(solid_image(4, [10, 20, 30, 255], 3));
        let bake = composite_region(BakeRegion::Hair, 4, &[base]);
        let decoded = bake.to_decoded_image();
        assert_eq!(decoded.width, 4);
        assert_eq!(decoded.height, 4);
        assert_eq!(decoded.components, 4);
        assert_eq!(decoded.discard_level, sl_proto::DiscardLevel::FULL);
        assert_eq!(decoded.pixels.as_ref(), bake.pixels.as_slice());
    }

    #[test]
    fn tex_gen_defaults_and_builds() {
        let layer = Layer::base(solid_image(2, [1, 2, 3, 255], 3));
        assert_eq!(layer.tex_gen, TexGen::Default);
        let planar = layer.with_tex_gen(TexGen::Planar);
        assert_eq!(planar.tex_gen, TexGen::Planar);
    }

    #[test]
    fn process_mask_alpha_thresholds_and_ramps() {
        let approx = |a: f32, b: f32| (a - b).abs() < 1.0e-6;
        // Hard threshold (domain 0): a mask value passes when `raw >= 1 - weight`.
        assert!(approx(process_mask_alpha(1.0, 0.5, 0.0), 1.0));
        assert!(approx(process_mask_alpha(0.0, 0.5, 0.0), 0.0));
        assert!(approx(process_mask_alpha(0.5, 0.5, 0.0), 1.0));
        // Smooth ramp (domain > 0): `raw / domain - (1 - domain) * (1 - weight) / domain`.
        // weight 1 → no offset, so the value passes straight through the ramp.
        assert!(approx(process_mask_alpha(0.5, 1.0, 0.5), 1.0));
        assert!(approx(process_mask_alpha(0.05, 1.0, 0.1), 0.5));
    }

    /// A garment (blend) layer bounded by a shape mask paints only where the mask
    /// is opaque; where the mask is transparent the underlying base shows through
    /// (R14 — the sleeve / pants-length cut-out that keeps fabric off hands/feet).
    #[test]
    fn shape_mask_bounds_a_garment_layer() {
        let base = Layer::base(solid_image(8, [255, 0, 0, 255], 3));
        let garment = |mask_value: u8| {
            Layer::blend(solid_image(8, [0, 0, 255, 255], 3)).with_masks(vec![ShapeMask {
                image: solid_image(8, [mask_value, mask_value, mask_value, 255], 1),
                // A hard-threshold mask at `1 - weight = 0.5`.
                weight: 0.5,
                domain: 0.0,
                multiply_blend: false,
            }])
        };
        // Mask fully opaque (white) → the blue garment paints over the red base.
        let covered = composite_region(BakeRegion::UpperBody, 8, &[base.clone(), garment(255)]);
        assert_eq!(centre_pixel(&covered), [0, 0, 255, 255]);
        // Mask fully transparent (black) → coverage 0, so the red base shows.
        let bare = composite_region(BakeRegion::UpperBody, 8, &[base, garment(0)]);
        assert_eq!(centre_pixel(&bare), [255, 0, 0, 255]);
    }

    /// Two multiplicative masks intersect (min): coverage survives only where both
    /// are opaque — the reference viewer's `param_alpha` accumulation.
    #[test]
    fn multiplicative_masks_intersect() {
        let base = Layer::base(solid_image(8, [255, 0, 0, 255], 3));
        // First mask additive (seeds coverage) white; second multiplicative black
        // → product 0 → the garment is fully carved away, the base shows.
        let garment = Layer::blend(solid_image(8, [0, 0, 255, 255], 3)).with_masks(vec![
            ShapeMask {
                image: solid_image(8, [255, 255, 255, 255], 1),
                weight: 0.5,
                domain: 0.0,
                multiply_blend: false,
            },
            ShapeMask {
                image: solid_image(8, [0, 0, 0, 255], 1),
                weight: 0.5,
                domain: 0.0,
                multiply_blend: true,
            },
        ]);
        let bake = composite_region(BakeRegion::UpperBody, 8, &[base, garment]);
        assert_eq!(centre_pixel(&bake), [255, 0, 0, 255]);
    }
}
