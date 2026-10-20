//! From a worn outfit to its baked images: the half of client-side baking that
//! needs no runtime, shared by the viewer (which bakes its own avatar on a grid
//! that does not) and the fake grid (which bakes every avatar, as Second Life
//! does).
//!
//! - [`assemble_layers`] walks every bake region's plan ([`region_layers`])
//!   against the worn wearable assets: each layer's texture, its tint from the
//!   wearable's colour parameters, and its garment-shape masks' weights.
//! - [`bake_region`] composites one region's layers into the image a baked
//!   texture holds — flipped to the bottom-up orientation avatar UVs are
//!   authored in, the eyes forced opaque.
//! - [`load_static_layers`] reads the `character/` TGAs the plan draws on (the
//!   skin grain, the sclera, the colour details, the garment-shape masks).
//!
//! Fetching the wearables and their textures stays with the caller: the
//! viewer streams them, the fake grid reads them out of its asset store.

use std::collections::HashMap;
use std::path::Path;

use sl_avatar::{
    AppearanceValues, ResolvedParams, VisualParams, WearableAsset, combine_layer_color,
    global_color,
};
use sl_proto::{DiscardLevel, WearableType, avatar_texture};
use sl_texture::DecodedImage;
use uuid::Uuid;

use crate::composite::{Layer, composite_region};
use crate::plan::{LayerTint, region_layers, shape_mask_files, static_layer_files};
use crate::region::BakeRegion;

/// The side of a baked image, in pixels: the reference's bake size.
pub const BAKE_SIZE: u32 = 512;

/// Bytes per RGBA8 texel.
const RGBA_CHANNELS: usize = 4;

/// Assembles every bake region's ordered layers from the worn wearable
/// `assets`, resolving each layer's tint and garment-shape mask weights
/// against the visual-parameter table `params` (white tints and default
/// weights where it is absent).
///
/// - `image_for(id)` — the decoded layer texture `id`, or `None` when it is
///   not in hand (that layer is then left out);
/// - `static_image(file)` — the decoded static `character/` TGA of that name
///   ([`load_static_layers`]).
#[must_use]
pub fn assemble_layers(
    assets: &[WearableAsset],
    params: Option<&VisualParams>,
    image_for: impl Fn(Uuid) -> Option<DecodedImage>,
    static_image: impl Fn(&str) -> Option<DecodedImage>,
) -> HashMap<BakeRegion, Vec<Layer>> {
    // Resolve each worn wearable's driver → driven params once (R14): a garment's
    // shape masks are keyed on driven params (sleeve length 600, pants length
    // 615, …) whose weight the wearable stores only as the group-0 driver
    // (Sleeve Length 800, Pants Length 815, …), so the mask weight must come from
    // the driver-propagated resolution, not the raw stored params.
    let resolved = resolve_wearable_params(assets, params);
    BakeRegion::ALL
        .into_iter()
        .map(|region| {
            let layers = region_layers(
                region,
                |wearable| assets.iter().any(|asset| asset.wearable_type == wearable),
                |slot| layer_image(assets, &image_for, slot),
                &static_image,
                |tint, wearable| layer_tint(assets, params, tint, wearable),
                |param_id, wearable| mask_weight(&resolved, params, param_id, wearable),
            );
            (region, layers)
        })
        .collect()
}

/// The layer texture ids the worn `assets` supply to `region`'s plan, in plan
/// order — what a grid's bake of the region is made of.
#[must_use]
pub fn region_texture_ids(region: BakeRegion, assets: &[WearableAsset]) -> Vec<Uuid> {
    crate::plan::region_plan(region)
        .iter()
        .filter_map(|planned| match planned.source {
            crate::plan::LayerSource::Wearable(slot) => layer_texture_id(assets, slot),
            _ => None,
        })
        .collect()
}

/// The texture id of the first worn wearable of `slot`'s type that supplies
/// it.
fn layer_texture_id(assets: &[WearableAsset], slot: usize) -> Option<Uuid> {
    let wearable_type = avatar_texture::layer_wearable_type(slot)?;
    assets
        .iter()
        .filter(|asset| asset.wearable_type == wearable_type)
        .find_map(|asset| asset.layer_texture(slot))
}

/// The decoded texture for a bake-region layer `slot`: the first worn wearable
/// of the slot's wearable type that supplies it, if its texture is in hand.
fn layer_image(
    assets: &[WearableAsset],
    image_for: impl Fn(Uuid) -> Option<DecodedImage>,
    slot: usize,
) -> Option<DecodedImage> {
    let wearable_type = avatar_texture::layer_wearable_type(slot)?;
    assets
        .iter()
        .filter(|asset| asset.wearable_type == wearable_type)
        .find_map(|asset| asset.layer_texture(slot).and_then(&image_for))
}

/// The linear-RGBA tint for a layer, resolved from the worn wearable's visual
/// params against the `avatar_lad.xml` table (opaque white when the table is
/// absent or the tint is [`LayerTint::White`]).
fn layer_tint(
    assets: &[WearableAsset],
    params: Option<&VisualParams>,
    tint: LayerTint,
    wearable: WearableType,
) -> [f32; 4] {
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let (LayerTint::Global(_) | LayerTint::Params(_)) = tint else {
        return WHITE;
    };
    let Some(params) = params else {
        return WHITE;
    };
    // The worn wearable of this layer's type supplies the colour param weights.
    let asset = assets.iter().find(|asset| asset.wearable_type == wearable);
    let weight_of = |id: i32| asset.and_then(|asset| asset.params.get(&id).copied());
    match tint {
        LayerTint::Global(name) => global_color(params, name, weight_of).unwrap_or(WHITE),
        LayerTint::Params(ids) => combine_layer_color(params, ids, weight_of),
        LayerTint::White => WHITE,
    }
}

/// Resolve each worn wearable's full param set — driver → driven propagation
/// (R14) — keyed by wearable type, so a garment's driven shape params (sleeve /
/// pants length, …) carry the weight derived from the wearable's stored group-0
/// driver. Empty when the visual-param table is not loaded.
fn resolve_wearable_params(
    assets: &[WearableAsset],
    params: Option<&VisualParams>,
) -> Vec<(WearableType, ResolvedParams)> {
    let Some(params) = params else {
        return Vec::new();
    };
    // `WearableType` is not `Hash`, and there are only a handful of worn
    // wearables, so a small assoc list (linear-searched below) suffices.
    assets
        .iter()
        .map(|asset| {
            let values = AppearanceValues::from_weights(
                asset.params.iter().map(|(&id, &weight)| (id, weight)),
            );
            (
                asset.wearable_type,
                ResolvedParams::from_values(params, &values),
            )
        })
        .collect()
}

/// The weight for a garment-shape mask's driving param (R14): the worn wearable's
/// driver-resolved weight for `param_id` (a driven shape param), falling back to
/// the visual-param table's default (then `0.0`). This is what carves a garment's
/// fabric to its sleeve / bottom / collar / pants-length extent so it does not
/// paint the bare hands and feet.
fn mask_weight(
    resolved: &[(WearableType, ResolvedParams)],
    params: Option<&VisualParams>,
    param_id: i32,
    wearable: WearableType,
) -> f32 {
    if let Some((_, resolved)) = resolved.iter().find(|(kind, _)| *kind == wearable)
        && let Some(weight) = resolved.weight(param_id)
    {
        return weight;
    }
    params
        .and_then(|params| params.get(param_id))
        .map_or(0.0, |param| param.default)
}

/// Composites one bake region from its `layers` into the image its baked
/// texture holds, or `None` when the region has no layers (an empty composite
/// is wholly transparent and would wrongly carve the region away).
///
/// The result is the orientation a Second Life baked texture is stored and
/// consumed in:
///
/// - **Vertical flip.** SL avatar `.llm` UVs are authored bottom-up (V = 0 at the
///   bottom), so the body samples a baked texture upside down relative to a
///   top-down decoded image. The compositor works top-down (like a fetched J2C),
///   which would land the head bake's chin/teeth on the forehead, so its rows are
///   flipped — matching how a server-published bake is stored (the reference
///   viewer bakes into a bottom-up GL surface).
/// - **Opaque eyes.** The eyeball is an opaque surface, but the simplified eye
///   composite carries only the iris layer (not the opaque sclera base the
///   reference eye layer-set builds), whose transparent surround would classify
///   the bake as masked and carve the eyeballs into empty sockets — so the eye
///   region is forced fully opaque.
#[must_use]
pub fn bake_region(region: BakeRegion, layers: &[Layer]) -> Option<DecodedImage> {
    if layers.is_empty() {
        return None;
    }
    let mut baked = composite_region(region, BAKE_SIZE, layers);
    let side = usize::try_from(BAKE_SIZE).unwrap_or(0);
    flip_rows_vertically(&mut baked.pixels, side, side);
    if region == BakeRegion::Eyes {
        force_alpha_opaque(&mut baked.pixels);
    }
    Some(baked.to_decoded_image())
}

/// Flip an RGBA8 image's rows in place — mirror it about its horizontal axis —
/// mapping between the top-down decoded-image row order and the OpenGL bottom-up
/// convention Second Life avatar UVs are authored in. A zero dimension, or a
/// pixel buffer too short for `width`×`height` RGBA, is left untouched.
pub const fn flip_rows_vertically(pixels: &mut [u8], width: usize, height: usize) {
    let stride = width.saturating_mul(RGBA_CHANNELS);
    // Guard the swaps: every index touched must be within the buffer.
    if stride == 0 || height == 0 || pixels.len() < stride.saturating_mul(height) {
        return;
    }
    let mut row = 0_usize;
    while row < height / 2 {
        let opposite = height.saturating_sub(1).saturating_sub(row);
        let top = row.saturating_mul(stride);
        let bottom = opposite.saturating_mul(stride);
        let mut offset = 0_usize;
        while offset < stride {
            pixels.swap(top.saturating_add(offset), bottom.saturating_add(offset));
            offset = offset.saturating_add(1);
        }
        row = row.saturating_add(1);
    }
}

/// Force every pixel of an RGBA8 image fully opaque (alpha byte → 255), so a
/// bake draped on a solid surface (the eyeball) is not carved by stray
/// source-texture transparency.
fn force_alpha_opaque(pixels: &mut [u8]) {
    let mut index = RGBA_CHANNELS.saturating_sub(1);
    while index < pixels.len() {
        // The alpha byte of each RGBA texel.
        if let Some(alpha) = pixels.get_mut(index) {
            *alpha = u8::MAX;
        }
        index = index.saturating_add(RGBA_CHANNELS);
    }
}

/// The static `character/` layer textures the bake plan draws on, as read
/// from one directory.
#[derive(Debug, Clone, Default)]
pub struct StaticLayers {
    /// Each decoded TGA, by file name.
    pub images: HashMap<&'static str, DecodedImage>,
    /// Each file that could not be read or decoded, with why. Its layer then
    /// falls back (a static base to a solid skin fill; a shape mask is not
    /// applied), so a caller reports these rather than failing.
    pub failures: Vec<(&'static str, String)>,
}

/// Reads and decodes every static layer TGA ([`static_layer_files`]) and
/// garment-shape mask ([`shape_mask_files`]) the bake plan names from `dir`
/// (a Linden `character/` directory).
#[must_use]
pub fn load_static_layers(dir: &Path) -> StaticLayers {
    let mut layers = StaticLayers::default();
    for file in static_layer_files().into_iter().chain(shape_mask_files()) {
        match fs_err::read(dir.join(file)) {
            Ok(bytes) => match decode_tga(&bytes) {
                Ok(decoded) => {
                    let _previous = layers.images.insert(file, decoded);
                }
                Err(error) => layers.failures.push((file, format!("decoding: {error}"))),
            },
            Err(error) => layers.failures.push((file, format!("reading: {error}"))),
        }
    }
    layers
}

/// Decode a TGA image's bytes into a [`DecodedImage`] (RGBA8), recording the
/// source component count so the compositor reads a colour-detail layer's alpha
/// but treats a grayscale skin-grain base as opaque.
fn decode_tga(bytes: &[u8]) -> Result<DecodedImage, image::ImageError> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Tga)?;
    let components = match image.color() {
        image::ColorType::L8 | image::ColorType::L16 => 1,
        image::ColorType::La8 | image::ColorType::La16 => 2,
        image::ColorType::Rgb8 | image::ColorType::Rgb16 | image::ColorType::Rgb32F => 3,
        _other => 4,
    };
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Ok(DecodedImage::new(
        width,
        height,
        components,
        DiscardLevel::FULL,
        bytes::Bytes::from(rgba.into_raw()),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::{flip_rows_vertically, force_alpha_opaque};
    use pretty_assertions::assert_eq;

    /// Rows are mirrored top to bottom; a buffer too short for its declared
    /// geometry is left as it is.
    #[test]
    fn flip_rows_vertically_mirrors_top_and_bottom() {
        // Row 0 red, row 1 blue (1 px wide, RGBA).
        let mut pixels = vec![255, 0, 0, 255, 0, 0, 255, 255];
        flip_rows_vertically(&mut pixels, 1, 2);
        assert_eq!(pixels, vec![0, 0, 255, 255, 255, 0, 0, 255]);
        let mut short = vec![1, 2, 3];
        flip_rows_vertically(&mut short, 1, 2);
        assert_eq!(short, vec![1, 2, 3]);
    }

    /// The eye-bake opacity forcing sets every texel's alpha byte to 255 while
    /// leaving the colour channels untouched, so a transparent-surround iris no
    /// longer carves the opaque eyeball away.
    #[test]
    fn force_alpha_opaque_fills_only_the_alpha_channel() {
        let mut pixels = vec![10, 20, 30, 0, 40, 50, 60, 128];
        force_alpha_opaque(&mut pixels);
        assert_eq!(pixels, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }
}
