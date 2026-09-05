//! Procedural test assets shared by every sl-client test tier.
//!
//! The no-grid render scenes build their textures as Bevy images and the fake
//! grid serves texture *bytes* by UUID; for a pixel oracle to mean the same
//! thing in both — "the checker is red and green" — the generator has to be
//! one. This crate is that generator: marker-coloured solids, checkers and
//! gradients as RGBA8, and the same images as JPEG2000 codestreams the fake
//! grid can serve over `GetTexture`.
//!
//! The marker colours match the pixel oracles' dominant-channel classes:
//! near-primary, `0.9` in the marker's channel(s) and `0.1` in the others, so a
//! rendered pixel of one is unambiguous and a blend of two reads as both.
//!
//! What the same fixtures need beside the pixels lives here too:
//! [`sculpt_sphere`] (a sculpt map, which is geometry stored *as* a texture),
//! [`mesh::unit_cube_mesh_asset`] (a whole mesh asset),
//! [`rigged::cylinder_mesh_asset`] (a *skinned* one, plus the
//! deliberately malformed rigs beside it),
//! [`anim::chest_twist_animation_asset`] (a keyframe motion),
//! [`gltf_material_asset`] (a PBR material asset),
//! [`sound::marker_tone`] (an Ogg Vorbis sound, whose pitch is to the audio
//! oracles what the marker colours are to the pixel ones) and
//! [`environment::night_sky_asset`] (an EEP settings asset, whose brightness is
//! to an environment oracle the same thing).
//!
//! [`inventory`] is the table that ties those to inventory:
//! [`inventory::seeded_assets`] is one real asset body per class a viewer can
//! hold, with the id an item declares and the bytes that id has to resolve to —
//! so a fixture grid's inventory item is openable rather than a number pointing
//! at nothing.
//!
//! [`builtin`] is the odd one out: not fixtures a test names, but stand-ins for
//! the Linden **library** textures a viewer asks every grid for on arrival — the
//! sun and moon discs, the cloud noise, the sky overlays, the wave normal and
//! the blank plywood. A fake grid serves those under their real ids, because a
//! grid with a library is exactly what it is pretending to be.

pub mod anim;
pub mod builtin;
pub mod environment;
pub mod inventory;
pub mod mesh;
pub mod rigged;
pub mod sound;

use bytes::Bytes;
use sl_proto::DEFAULT_TERRAIN_DETAIL_TEXTURES;
use sl_proto::j2c::DiscardLevel;
use sl_texture::{DecodedImage, EncodeError, encode_baked_avatar_j2c, encode_j2c};
use uuid::Uuid;

/// The marker colours the oracles classify by dominant channel.
pub mod markers {
    /// `(0.9, 0.1, 0.1)`.
    pub const RED: [u8; 4] = [230, 25, 25, 255];
    /// `(0.1, 0.9, 0.1)`.
    pub const GREEN: [u8; 4] = [25, 230, 25, 255];
    /// `(0.1, 0.1, 0.9)`.
    pub const BLUE: [u8; 4] = [25, 25, 230, 255];
    /// `(0.9, 0.9, 0.1)`.
    pub const YELLOW: [u8; 4] = [230, 230, 25, 255];
    /// A dark neutral below every oracle's presence threshold — what the sea
    /// and the sky are, and what a backdrop must be to count as *nothing*.
    pub const DARK: [u8; 4] = [40, 40, 40, 255];
}

/// An RGBA8 image, row-major, `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels.
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// A `size`×`size` image painted by `paint(x, y)`.
    pub(crate) fn painted(size: u32, paint: impl Fn(u32, u32) -> [u8; 4]) -> Self {
        let mut pixels = Vec::new();
        for y in 0..size {
            for x in 0..size {
                pixels.extend_from_slice(&paint(x, y));
            }
        }
        Self {
            width: size,
            height: size,
            pixels,
        }
    }

    /// A `size`×`size` image of one colour.
    #[must_use]
    pub fn solid(size: u32, rgba: [u8; 4]) -> Self {
        Self::painted(size, |_x, _y| rgba)
    }

    /// A `size`×`size` checkerboard of `cell`-pixel squares, `a` in the corner
    /// square and `b` in its neighbours. A `cell` of zero paints all `a`.
    #[must_use]
    pub fn checker(size: u32, cell: u32, a: [u8; 4], b: [u8; 4]) -> Self {
        Self::painted(size, |x, y| {
            if cell == 0 {
                return a;
            }
            let column = x.checked_div(cell).unwrap_or(0);
            let row = y.checked_div(cell).unwrap_or(0);
            let odd = column
                .wrapping_add(row)
                .checked_rem(2)
                .is_some_and(|parity| parity == 1);
            if odd { b } else { a }
        })
    }

    /// A `size`×`size` horizontal gradient from `from` at the left edge to `to`
    /// at the right edge.
    #[must_use]
    pub fn gradient(size: u32, from: [u8; 4], to: [u8; 4]) -> Self {
        let span = size.saturating_sub(1).max(1);
        Self::painted(size, |x, _y| {
            let mut rgba = [0_u8; 4];
            for (channel, (start, end)) in rgba.iter_mut().zip(from.into_iter().zip(to)) {
                // Integer interpolation in `u32` so no channel can overflow.
                let start = u32::from(start);
                let end = u32::from(end);
                let step = |delta: u32| delta.saturating_mul(x).checked_div(span).unwrap_or(0);
                let value = if end >= start {
                    start.saturating_add(step(end.saturating_sub(start)))
                } else {
                    start.saturating_sub(step(start.saturating_sub(end)))
                };
                *channel = u8::try_from(value).unwrap_or(u8::MAX);
            }
            rgba
        })
    }

    /// The pixel at `(x, y)`, or `None` outside the image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = usize::try_from(y)
            .ok()?
            .checked_mul(usize::try_from(self.width).ok()?)?
            .checked_add(usize::try_from(x).ok()?)?
            .checked_mul(4)?;
        let texel = self.pixels.get(index..index.checked_add(4)?)?;
        match texel {
            [r, g, b, a] => Some([*r, *g, *b, *a]),
            _other => None,
        }
    }

    /// The image as the texture pipeline's decoded form (four components, full
    /// resolution).
    #[must_use]
    pub fn into_decoded(self) -> DecodedImage {
        DecodedImage::new(
            self.width,
            self.height,
            4,
            DiscardLevel::FULL,
            Bytes::from(self.pixels),
            None,
        )
    }

    /// The image as a JPEG2000 codestream — what a grid serves over
    /// `GetTexture` and what the texture store decodes.
    ///
    /// # Errors
    ///
    /// Returns the encoder's error for an empty image.
    pub fn j2c(&self) -> Result<Vec<u8>, EncodeError> {
        encode_j2c(&self.clone().into_decoded())
    }

    /// The image as one **baked avatar texture**: the five-component
    /// (`R G B alpha mask`) JPEG2000 codestream a grid serves a baked avatar
    /// slot as, with an all-`255` morph mask.
    ///
    /// `255` is what the reference viewer's `gatherMorphMaskAlpha` starts the
    /// mask at before each worn layer subtracts its own coverage, so it is
    /// exactly the mask of a bake with no masking clothing layer — which is
    /// what a fixture avatar wears.
    ///
    /// A bake is not an ordinary texture and cannot be encoded as one. The
    /// reference viewer reads the fifth plane back as the morph mask for the
    /// head, upper-body and lower-body bakes, and a bake without one makes that
    /// read fail — which makes the viewer discard the colour decode along with
    /// it and mark the texture a missing asset, leaving the agent's own avatar
    /// a cloud however good the pixels were.
    ///
    /// # Errors
    ///
    /// Returns the encoder's error for an empty image.
    pub fn baked_avatar_j2c(&self) -> Result<Vec<u8>, EncodeError> {
        let texels = usize::try_from(self.width)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(self.height).unwrap_or(0));
        encode_baked_avatar_j2c(&self.clone().into_decoded(), &vec![u8::MAX; texels])
    }
}

/// A `size`×`size` **sculpt map** of a sphere in the convention real sculpt
/// content uses: the north pole (`z = +0.5`, blue at full) on the visible top
/// row and longitude running counter-clockwise (`+X` → `+Y`) across the
/// columns, which is the orientation the reference viewer tessellates outward.
///
/// A sculpt map is geometry stored as a texture — each pixel's `(r, g, b) /
/// 255 - 0.5` is a vertex position — so it belongs beside the other procedural
/// pixels rather than beside the mesh asset.
#[must_use]
pub fn sculpt_sphere(size: u32) -> RgbaImage {
    let span = |value: u32| f32::from(u16::try_from(value).unwrap_or(u16::MAX));
    RgbaImage::painted(size, |x, y| {
        let theta = core::f32::consts::PI * span(y) / span(size.saturating_sub(1)).max(1.0);
        let phi = core::f32::consts::TAU * span(x) / span(size).max(1.0);
        let channel = |value: f32| round_to_u8((0.5 + 0.5 * value) * 255.0);
        [
            channel(theta.sin() * phi.cos()),
            channel(theta.sin() * phi.sin()),
            channel(theta.cos()),
            255,
        ]
    })
}

/// Rounds a value already inside `0..=255` to its byte.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=255 before the cast; no From impl exists"
)]
pub(crate) const fn round_to_u8(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// The `u16` full-scale value every quantized stream this crate writes is taken
/// over: the mesh streams' `dequantize` and the animation keyframes' widening
/// both divide by exactly this.
const U16_SCALE: f32 = 65_535.0;

/// Quantizes `value` in `[min, max]` to the `u16` sample the decoders'
/// dequantisation (`min + sample / 65535 * (max - min)`) inverts. A degenerate
/// range quantizes to zero rather than dividing by it.
fn quantize_u16(value: f32, min: f32, max: f32) -> u16 {
    let span = max - min;
    if span <= 0.0 {
        return 0;
    }
    round_to_u16(((value - min) / span).clamp(0.0, 1.0) * U16_SCALE)
}

/// Rounds a value already clamped into `0..=65535` to its `u16`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=65535 before the cast; no From impl exists"
)]
const fn round_to_u16(value: f32) -> u16 {
    value.round().clamp(0.0, U16_SCALE) as u16
}

/// Appends `value` little-endian, the byte order every quantized stream this
/// crate writes uses.
#[expect(
    clippy::little_endian_bytes,
    reason = "the mesh streams and the animation keyframes are wire-defined little-endian"
)]
fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// The `AT_MATERIAL` envelope fields a GLTF material asset is wrapped in
/// (`LLGLTFMaterial::ASSET_TYPE` and its newest accepted version).
const GLTF_ASSET_TYPE: &str = "GLTF 2.0";
/// The asset-envelope version written below.
const GLTF_ASSET_VERSION: &str = "1.1";

/// A **GLTF (PBR) material asset** — what the `ViewerAsset` capability serves
/// for the `material_id` an object's `RenderMaterial` extra-params block
/// names: the LLSD envelope `{ version, type, data }` whose `data` is a glTF
/// 2.0 document carrying one material.
///
/// `base_color` is the linear RGBA factor and `base_color_texture` the texture
/// asset the material samples, if any. The material is fully rough and
/// non-metallic, so what a fixture asserts about it is its colour.
#[must_use]
pub fn gltf_material_asset(base_color: [f32; 4], base_color_texture: Option<Uuid>) -> Vec<u8> {
    let factor = base_color
        .iter()
        .map(|component| component.to_string())
        .collect::<Vec<String>>()
        .join(", ");
    let (texture_slot, indirection) = match base_color_texture {
        Some(id) => (
            r#", "baseColorTexture": { "index": 0 }"#.to_owned(),
            format!(r#", "textures": [ {{ "source": 0 }} ], "images": [ {{ "uri": "{id}" }} ]"#),
        ),
        None => (String::new(), String::new()),
    };
    let document = format!(
        r#"{{ "asset": {{ "version": "2.0" }}, "materials": [ {{ "pbrMetallicRoughness": {{ "baseColorFactor": [ {factor} ], "metallicFactor": 0.0, "roughnessFactor": 1.0{texture_slot} }} }} ]{indirection} }}"#
    );
    sl_llsd::Llsd::Map(
        [
            (
                "version".to_owned(),
                sl_llsd::Llsd::String(GLTF_ASSET_VERSION.to_owned()),
            ),
            (
                "type".to_owned(),
                sl_llsd::Llsd::String(GLTF_ASSET_TYPE.to_owned()),
            ),
            ("data".to_owned(), sl_llsd::Llsd::String(document)),
        ]
        .into_iter()
        .collect(),
    )
    .to_llsd_binary()
}

/// The side, in pixels, of the terrain detail textures below.
const TERRAIN_DETAIL_SIZE: u32 = 32;

/// One flat JPEG2000 texture per default Linden terrain detail slot (the four
/// UUIDs a region handshake names when it does not name its own), so a fake
/// region's ground is four distinguishable solids rather than four failed
/// fetches.
///
/// # Errors
///
/// Returns the encoder's error, which a 32×32 solid cannot produce.
pub fn terrain_detail_solids() -> Result<[(Uuid, Vec<u8>); 4], EncodeError> {
    let colours = [
        [110, 90, 60, 255],
        [70, 120, 50, 255],
        [130, 130, 120, 255],
        [200, 200, 210, 255],
    ];
    let mut out = Vec::new();
    for (id, rgba) in DEFAULT_TERRAIN_DETAIL_TEXTURES.into_iter().zip(colours) {
        out.push((id, RgbaImage::solid(TERRAIN_DETAIL_SIZE, rgba).j2c()?));
    }
    out.try_into().map_err(|_four| EncodeError::Empty)
}

#[cfg(test)]
mod tests {
    use super::{RgbaImage, markers, sculpt_sphere, terrain_detail_solids};
    use pretty_assertions::assert_eq;
    use sl_proto::j2c::DiscardLevel;
    use sl_texture::decode_j2c;

    type TestError = Box<dyn core::error::Error>;

    #[test]
    fn a_checker_alternates_by_cell() {
        let image = RgbaImage::checker(8, 2, markers::RED, markers::GREEN);
        assert_eq!(image.pixel(0, 0), Some(markers::RED));
        assert_eq!(image.pixel(1, 1), Some(markers::RED));
        assert_eq!(image.pixel(2, 0), Some(markers::GREEN));
        assert_eq!(image.pixel(0, 2), Some(markers::GREEN));
        assert_eq!(image.pixel(2, 2), Some(markers::RED));
        assert_eq!(image.pixel(8, 0), None);
        // A zero cell is a solid.
        assert_eq!(
            RgbaImage::checker(4, 0, markers::RED, markers::GREEN),
            RgbaImage::solid(4, markers::RED)
        );
    }

    #[test]
    fn a_gradient_runs_from_its_left_edge_to_its_right() {
        let image = RgbaImage::gradient(5, [0, 100, 255, 255], [200, 100, 55, 255]);
        assert_eq!(image.pixel(0, 0), Some([0, 100, 255, 255]));
        assert_eq!(image.pixel(4, 0), Some([200, 100, 55, 255]));
        assert_eq!(image.pixel(2, 3), Some([100, 100, 155, 255]));
    }

    /// The codestream decodes back to the image it was made from, so what the
    /// fake grid serves is what the render scenes paint.
    #[test]
    fn a_checker_survives_the_jpeg2000_round_trip() -> Result<(), TestError> {
        let image = RgbaImage::checker(64, 16, markers::RED, markers::GREEN);
        let decoded = decode_j2c(&image.j2c()?, DiscardLevel::FULL)?;
        assert_eq!((decoded.width, decoded.height), (64, 64));
        // The middle of a cell, well clear of any codec ringing at the edges.
        for (x, y, expected) in [(8, 8, markers::RED), (24, 8, markers::GREEN)] {
            let index = usize::try_from(y * 64 + x)
                .ok()
                .and_then(|i| i.checked_mul(4))
                .ok_or("index")?;
            let texel = decoded
                .pixels
                .get(index..index.saturating_add(4))
                .ok_or("texel")?;
            for (got, want) in texel.iter().zip(expected) {
                assert!(
                    got.abs_diff(want) <= 8,
                    "pixel ({x}, {y}) decoded to {texel:?}, wanted {expected:?}"
                );
            }
        }
        Ok(())
    }

    /// The poles and the equator sit where the sculpt sampler expects them:
    /// the top row is `z = +0.5` (blue at full), the bottom row `z = -0.5`,
    /// and the equator runs `+X` → `+Y` across the columns.
    #[test]
    fn the_sculpt_sphere_puts_its_north_pole_on_the_top_row() -> Result<(), TestError> {
        let map = sculpt_sphere(16);
        let blue = |x, y| map.pixel(x, y).map(|[_r, _g, b, _a]| b);
        assert_eq!(blue(0, 0), Some(255));
        assert_eq!(blue(7, 0), Some(255));
        assert_eq!(blue(0, 15), Some(0));
        // Row 8 of 16 is the sample closest to the equator (θ = π/2): its
        // first column points at +X, a quarter of the way round at +Y.
        let [red, _g, _b, _a] = map.pixel(0, 8).ok_or("no equator sample")?;
        assert!(red > 200, "column 0 of the equator is not +X (red {red})");
        let [_r, green, _b, _a] = map.pixel(4, 8).ok_or("no quarter sample")?;
        assert!(
            green > 200,
            "column 4 of the equator is not +Y (green {green})"
        );
        Ok(())
    }

    /// The material asset decodes back through the viewer's own material
    /// decoder into the colour and texture it was written with.
    #[expect(
        clippy::float_cmp,
        reason = "the factors travel as exactly-representable decimals through \
                  the JSON, so exact equality is the test"
    )]
    #[test]
    fn a_gltf_material_asset_decodes_back() -> Result<(), TestError> {
        let texture = uuid::Uuid::from_u128(0x00C0_FFEE);
        let asset = super::gltf_material_asset([0.25, 0.5, 0.75, 1.0], Some(texture));
        let material = sl_material::parse_material_asset(&asset)?;
        assert_eq!(material.base_color, [0.25_f32, 0.5, 0.75, 1.0]);
        assert_eq!(
            material.base_color_texture.map(|slot| slot.id.uuid()),
            Some(texture)
        );
        // Without a texture the material is a plain colour.
        let plain = super::gltf_material_asset([1.0, 0.0, 0.0, 1.0], None);
        let plain = sl_material::parse_material_asset(&plain)?;
        assert_eq!(plain.base_color, [1.0, 0.0, 0.0, 1.0]);
        assert!(plain.base_color_texture.is_none());
        Ok(())
    }

    #[test]
    fn the_terrain_detail_solids_cover_the_four_default_slots() -> Result<(), TestError> {
        let solids = terrain_detail_solids()?;
        let ids: Vec<_> = solids.iter().map(|(id, _bytes)| *id).collect();
        assert_eq!(ids, sl_proto::DEFAULT_TERRAIN_DETAIL_TEXTURES.to_vec());
        for (_id, bytes) in &solids {
            let decoded = decode_j2c(bytes, DiscardLevel::FULL)?;
            assert_eq!((decoded.width, decoded.height), (32, 32));
        }
        Ok(())
    }
}
