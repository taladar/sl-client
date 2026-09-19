//! The in-world **Land Owners** tint (`viewer-parcel-owners-terrain-overlay`):
//! the ground itself shaded by parcel-ownership class, the reference viewer's
//! `ShowParcelOwners` (World ▸ Show More ▸ Land Owners, and the Land tool's
//! *Show owners* checkbox).
//!
//! The reference draws it as a second, alpha-blended pass over the terrain
//! (`LLDrawPoolTerrain::renderOwnership`) textured with
//! `LLViewerParcelOverlay`'s own 64×64 RGBA image — one texel per 4 m
//! parcel-overlay square, coloured by that square's ownership class. We do the
//! same thing in one pass instead of two: each region's
//! [`TerrainMaterial`] carries that image plus a
//! [`TerrainOwnership`] uniform, and its fragment shader mixes the tint over the
//! lit ground.
//!
//! Two things are therefore kept in step here, both change-driven:
//!
//! - **The image**, rebuilt from a region's decoded [`ParcelOverlayGrid`]
//!   whenever that grid changes (parcels split, join or go on sale). The colours
//!   are the ones the property lines already use (`parcel_borders`'s own
//!   `ownership_color`), so a boundary line and the ground inside it never
//!   disagree.
//! - **The uniform**, whose `strength` is the `ShowParcelOwners` switch and
//!   whose `uv_scale` maps the mesh's tiled detail UV onto the whole region.
//!   Toggling the overlay writes this one uniform rather than touching any
//!   image, and nothing here runs at all while neither the setting nor a grid
//!   has moved — a material rewritten per frame would rebuild its bind group
//!   per frame.
//!
//! Reference (Firestorm, read-only): `lldrawpoolterrain.cpp`
//! (`renderOwnership`), `llviewerparceloverlay.cpp`
//! (`updateOverlayTexture`), `llpanelland.cpp`, `menu_viewer.xml` L1243.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use sl_client_bevy::{
    DEFAULT_REGION_WIDTH_METRES, DETAIL_TILE_METRES, ParcelOverlayGrid, RegionHandle,
    SlParcelOverlay, TerrainMaterial, TerrainOwnership,
};
use sl_viewer_settings::ViewerSettings;

use crate::parcel_borders::{SETTING_SHOW_PARCEL_OWNERS, ownership_color};
use crate::terrain::TerrainTextures;

/// Side length, in metres, of one parcel-overlay grid square (the reference's
/// `PARCEL_GRID_STEP_METERS`) — one texel of the ownership map.
const PARCEL_GRID_STEP_METRES: f32 = 4.0;

/// How opaque the ownership tint is over the ground. The reference's
/// `PropertyColor*` settings carry `0.4`; the same value here keeps the ground
/// texture legible through the class colour rather than flooding it.
const OVERLAY_ALPHA: u8 = 102;

/// How many bytes one RGBA texel takes.
const RGBA_BYTES: usize = 4;

/// The per-region ownership maps and what they were built from, so neither the
/// image nor the material is rewritten while nothing has changed.
#[derive(Resource, Debug, Default)]
pub struct ParcelOwnerOverlay {
    /// Each region's ownership map.
    images: HashMap<RegionHandle, Handle<Image>>,
    /// The grid each region's map was built from, so an unchanged grid rebuilds
    /// nothing. Held by value (an overlay grid is a few KiB) because the compare
    /// is what makes this change-driven.
    built_from: HashMap<RegionHandle, ParcelOverlayGrid>,
    /// The 1×1 fully transparent map every region's material starts with, so a
    /// region whose overlay has not arrived is bound to something valid.
    blank: Option<Handle<Image>>,
    /// Whether the tint was on last time the materials were written.
    active: bool,
}

/// Build and bind each region's parcel-ownership map, and switch the tint on and
/// off — the whole of the Land Owners overlay. See the
/// [module documentation](self).
fn update_parcel_owner_overlay(
    settings: Option<Res<ViewerSettings>>,
    overlay: Res<SlParcelOverlay>,
    textures: Res<TerrainTextures>,
    mut state: ResMut<ParcelOwnerOverlay>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let show = settings.is_some_and(|settings| {
        settings
            .store()
            .get_bool(SETTING_SHOW_PARCEL_OWNERS)
            .unwrap_or(false)
    });
    // Nothing to do while the switch has not moved and no grid has: rewriting a
    // material re-prepares its bind group, so this must not run per frame.
    let materials_changed = textures.is_changed();
    if show == state.active && !overlay.is_changed() && !materials_changed {
        return;
    }
    if !show {
        if state.active {
            state.active = false;
            for handle in textures.material_handles() {
                if let Some(mut material) = materials.get_mut(&handle) {
                    material.ownership.strength = 0.0;
                }
            }
        }
        return;
    }

    let blank = state
        .blank
        .get_or_insert_with(|| images.add(blank_map()))
        .clone();
    for (region, handle) in textures.materials_by_region() {
        let grid = overlay.grid_of(region);
        // Rebuild this region's map only when its grid is new or has moved.
        if let Some(grid) = grid
            && state.built_from.get(&region) != Some(grid)
        {
            let image = images.add(ownership_map(grid));
            state.images.insert(region, image);
            state.built_from.insert(region, grid.clone());
        }
        let map = state
            .images
            .get(&region)
            .cloned()
            .unwrap_or_else(|| blank.clone());
        let width = grid.map_or(DEFAULT_REGION_WIDTH_METRES, region_width_metres);
        let Some(mut material) = materials.get_mut(&handle) else {
            continue;
        };
        let wanted = TerrainOwnership {
            uv_scale: DETAIL_TILE_METRES / width,
            strength: 1.0,
            ..TerrainOwnership::default()
        };
        if material.ownership_map != map {
            material.ownership_map = map;
        }
        if material.ownership.strength.to_bits() != wanted.strength.to_bits()
            || material.ownership.uv_scale.to_bits() != wanted.uv_scale.to_bits()
        {
            material.ownership = wanted;
        }
    }
    state.active = true;
}

/// A region's width in metres, from the side length of its overlay grid.
fn region_width_metres(grid: &ParcelOverlayGrid) -> f32 {
    // Via `u16` rather than a cast: `grids_per_edge` is 64 for a region and a
    // small multiple of that for a var-region, so the widening is exact — the
    // same hop `parcel_borders::grid_metres` makes.
    f32::from(u16::try_from(grid.grids_per_edge()).unwrap_or(u16::MAX)) * PARCEL_GRID_STEP_METRES
}

/// The 1×1 fully transparent map a region with no decoded overlay is bound to.
/// Transparent rather than absent: the binding must exist for the pipeline, and
/// a zero alpha makes the shader's mix a no-op.
fn blank_map() -> Image {
    let mut image = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0, 0, 0, 0],
        // Not `*Srgb`: the tint is a flat class colour mixed into an already-lit,
        // already-linear result, so it must not be de-gamma'd on the way in.
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = overlay_sampler();
    image
}

/// Build a region's ownership map from its decoded overlay grid: one RGBA texel
/// per 4 m square, row 0 southernmost — the row order
/// [`ParcelOverlayGrid::cells`] yields and the terrain's own V direction.
fn ownership_map(grid: &ParcelOverlayGrid) -> Image {
    let edge = grid.grids_per_edge();
    let mut data = vec![0_u8; edge.saturating_mul(edge).saturating_mul(RGBA_BYTES)];
    for (row, col, cell) in grid.cells() {
        let [red, green, blue] = ownership_color(cell.ownership);
        let Some(offset) = row
            .checked_mul(edge)
            .and_then(|index| index.checked_add(col))
            .and_then(|index| index.checked_mul(RGBA_BYTES))
        else {
            continue;
        };
        let Some(texel) = data.get_mut(offset..offset.saturating_add(RGBA_BYTES)) else {
            continue;
        };
        texel.copy_from_slice(&[channel(red), channel(green), channel(blue), OVERLAY_ALPHA]);
    }
    let edge = u32::try_from(edge).unwrap_or(1);
    let mut image = Image::new(
        Extent3d {
            width: edge,
            height: edge,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = overlay_sampler();
    image
}

/// One ownership colour channel as a byte (the shape `beacons::quantise` uses).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a clamped 0..=1 value scaled by 255 is whole after rounding and inside \
              a byte, so the conversion is exact"
)]
fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// How the ownership map is sampled: linearly, so the class colours blend
/// across a parcel boundary the way the reference's overlay texture does rather
/// than showing 4 m stair-steps, and clamped, so the region's outermost row and
/// column do not wrap around to the opposite edge.
fn overlay_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..ImageSamplerDescriptor::default()
    })
}

/// The in-world Land Owners tint.
#[derive(Debug, Default)]
pub struct ParcelOwnerOverlayPlugin;

impl Plugin for ParcelOwnerOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParcelOwnerOverlay>()
            // After the terrain update, so a region's material exists (and its
            // overlay grid has been folded in) by the time the tint is bound.
            .add_systems(
                Update,
                update_parcel_owner_overlay.after(crate::terrain::update_terrain),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::{OVERLAY_ALPHA, RGBA_BYTES, channel, ownership_map, region_width_metres};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ParcelOverlayGrid, ParcelOwnership};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A channel byte saturates rather than wrapping at either end.
    #[test]
    fn a_colour_channel_clamps_to_a_byte() {
        assert_eq!(channel(0.0), 0);
        assert_eq!(channel(1.0), 255);
        assert_eq!(channel(-1.0), 0);
        assert_eq!(channel(4.0), 255);
    }

    /// The map is one texel per 4 m square, and a region's width follows its
    /// grid — so a var-region's tint is not squeezed into its south-west
    /// quarter.
    #[test]
    fn the_map_is_one_texel_per_grid_square() -> Result<(), TestError> {
        let grid = ParcelOverlayGrid::new(64);
        assert_eq!(region_width_metres(&grid).to_bits(), 256.0_f32.to_bits());
        let image = ownership_map(&grid);
        assert_eq!(image.width(), 64);
        assert_eq!(image.height(), 64);
        let data = image.data.as_ref().ok_or("the map carries its texels")?;
        assert_eq!(data.len(), 64 * 64 * RGBA_BYTES);
        Ok(())
    }

    /// Every texel carries the overlay's alpha, so a square nobody owns is
    /// tinted (the reference greys public land) rather than left a hole.
    #[test]
    fn every_texel_is_tinted() -> Result<(), TestError> {
        let grid = ParcelOverlayGrid::new(8);
        let image = ownership_map(&grid);
        let data = image.data.as_ref().ok_or("the map carries its texels")?;
        for texel in data.as_chunks::<RGBA_BYTES>().0 {
            assert_eq!(texel.get(3).copied(), Some(OVERLAY_ALPHA));
        }
        // An unfilled grid decodes as public land, which the property lines
        // draw grey — the ground agrees with them.
        let grey = super::ownership_color(ParcelOwnership::Public);
        assert_eq!(data.first().copied(), Some(channel(grey[0])));
        Ok(())
    }
}
