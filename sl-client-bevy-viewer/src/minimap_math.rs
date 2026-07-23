//! Pure minimap ("net map") math and rasterisation — no Bevy, no I/O.
//!
//! Everything geometric or pixel-pushing about the minimap lives here so it can
//! be unit-tested without a window: the world↔widget transform (scale, rotation,
//! pan), the cached-layer raster sizing rule, the object / parcel layer
//! rasterisers, the compass-label edge placement, the avatar dot glyph and
//! colour selection, and the frustum-wedge / ring overlay painters.
//!
//! Ported from the reference viewer (Firestorm `llnetmap.cpp`,
//! `llfloatermap.cpp`, `llviewerobjectlist.cpp renderObjectsForMap` — read-only
//! reference), with the coordinate conventions translated once, here:
//!
//! - **World-relative metres**: `east` / `north` offsets from the camera
//!   position (the map is camera-centred, like the reference).
//! - **Surface pixels**: the composited map image, origin **top-left**, `y`
//!   down (a Bevy `Image` / UI convention; the reference's GL frame has `y` up,
//!   so the flip happens inside [`MapView`], not in every caller).
//! - **Layer rasters**: bottom-up rows (row `0` = south), exactly like the
//!   reference's `LLImageRaw`, so the layer-painting loops port verbatim; the
//!   compositor flips while sampling.

use bevy::math::Vec2;

/// The width of a (classic) region in metres — the unit [`MapView::scale`] is
/// expressed against.
pub(crate) const REGION_WIDTH_METRES: f32 = 256.0;

/// The smallest allowed map scale, in pixels per region (fully zoomed out).
pub(crate) const MAP_SCALE_MIN: f32 = 32.0;

/// The largest allowed map scale, in pixels per region (fully zoomed in).
pub(crate) const MAP_SCALE_MAX: f32 = 4096.0;

/// The "Very Close" zoom preset (pixels per region).
pub(crate) const MAP_SCALE_VERY_CLOSE: f32 = 1024.0;

/// The "Close" zoom preset (pixels per region).
pub(crate) const MAP_SCALE_CLOSE: f32 = 256.0;

/// The "Medium" zoom preset (pixels per region) — the default scale.
pub(crate) const MAP_SCALE_MEDIUM: f32 = 128.0;

/// The "Far" zoom preset (pixels per region).
pub(crate) const MAP_SCALE_FAR: f32 = 32.0;

/// The per-notch scroll-wheel zoom factor (4 % per click).
pub(crate) const MAP_SCALE_ZOOM_FACTOR: f32 = 1.04;

/// Avatar dot radius per pixel-per-metre (the reference's `DOT_SCALE`).
pub(crate) const DOT_SCALE: f32 = 0.75;

/// The smallest avatar dot radius, in pixels.
pub(crate) const MIN_DOT_RADIUS: f32 = 3.5;

/// Diagonal compass labels hide when their box would cover more than this
/// fraction of the map's smaller edge (`MAP_MINOR_DIR_THRESHOLD`).
pub(crate) const MINOR_DIR_THRESHOLD: f32 = 0.07;

/// The relative-height band (± metres) within which an avatar draws as a level
/// dot rather than an above / below chevron.
pub(crate) const HEIGHT_CUE_BAND: f32 = 7.0;

/// The coarse-location altitude ceiling in metres: a coarse `z` at (or above)
/// this — or at zero — means the simulator could not encode the altitude.
pub(crate) const COARSE_MAX_Z: f32 = 1020.0;

/// One RGBA colour, straight (non-premultiplied) alpha.
pub(crate) type Rgba = [u8; 4];

// ---------------------------------------------------------------------------
// The world ↔ surface transform.
// ---------------------------------------------------------------------------

/// The minimap's view state: one scale for all instances, the rotation baked
/// into both transforms, the pan offset, and the surface size in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MapView {
    /// Pixels per 256 m region (the persisted `MiniMapScale`).
    pub(crate) scale: f32,
    /// The rotation applied to the whole map about its centre, in radians.
    /// `0` = north-up; with rotate-on this is `atan2(at_east, at_north)` of the
    /// camera's at-axis so the camera heading points up.
    pub(crate) rotation: f32,
    /// The pan offset in surface pixels, `+x` = content shifted toward the
    /// right edge, `+y` = content shifted toward the *top* edge (the
    /// reference's GL-frame `mCurPan`).
    pub(crate) pan: Vec2,
    /// The surface size in pixels.
    pub(crate) size: Vec2,
}

impl MapView {
    /// Pixels per metre at the current scale.
    pub(crate) fn pixels_per_metre(&self) -> f32 {
        self.scale / REGION_WIDTH_METRES
    }

    /// A camera-relative world offset (metres east / north) as a surface pixel
    /// position (origin top-left, `y` down) — the reference's
    /// `globalPosToView`.
    pub(crate) fn view_from_rel(&self, east: f32, north: f32) -> Vec2 {
        let ppm = self.pixels_per_metre();
        let px = east * ppm;
        let py = north * ppm;
        let (sin, cos) = self.rotation.sin_cos();
        let rx = px * cos - py * sin;
        let ry = px * sin + py * cos;
        let gl_x = self.size.x / 2.0 + self.pan.x + rx;
        let gl_y = self.size.y / 2.0 + self.pan.y + ry;
        Vec2::new(gl_x, self.size.y - gl_y)
    }

    /// A surface pixel position (origin top-left, `y` down) as a
    /// camera-relative world offset in metres — the inverse of
    /// [`view_from_rel`](Self::view_from_rel), the reference's
    /// `viewPosToGlobal`.
    pub(crate) fn rel_from_view(&self, view: Vec2) -> (f32, f32) {
        let gl_x = view.x - self.size.x / 2.0 - self.pan.x;
        let gl_y = (self.size.y - view.y) - self.size.y / 2.0 - self.pan.y;
        let (sin, cos) = (-self.rotation).sin_cos();
        let px = gl_x * cos - gl_y * sin;
        let py = gl_x * sin + gl_y * cos;
        let ppm = self.pixels_per_metre();
        (px / ppm, py / ppm)
    }
}

/// The map rotation for a camera at-axis, in radians: `atan2(at_east,
/// at_north)`, so that with rotate-on the camera heading points to the top of
/// the map (the reference's rotation formula).
pub(crate) fn rotation_for_camera(at_east: f32, at_north: f32) -> f32 {
    at_east.atan2(at_north)
}

/// Clamp a requested scale into the allowed range.
pub(crate) const fn clamp_scale(scale: f32) -> f32 {
    scale.clamp(MAP_SCALE_MIN, MAP_SCALE_MAX)
}

/// The new scale after `clicks` scroll-wheel notches (positive clicks zoom
/// out, matching the reference's reversed sense), clamped.
pub(crate) fn wheel_scale(scale: f32, clicks: f32) -> f32 {
    clamp_scale(scale * MAP_SCALE_ZOOM_FACTOR.powf(-clicks))
}

/// Rescale the pan offset when the scale changes so the view stays anchored
/// (the reference's `setScale` does `mCurPan *= new / old`).
pub(crate) fn rescale_pan(pan: Vec2, old_scale: f32, new_scale: f32) -> Vec2 {
    if old_scale <= 0.0 {
        return pan;
    }
    let factor = new_scale / old_scale;
    Vec2::new(pan.x * factor, pan.y * factor)
}

/// The pan adjustment that keeps the point under the cursor fixed across a
/// zoom, when auto-centring is off. `cursor` is in surface pixels (top-left
/// origin); returns the *new* pan.
pub(crate) fn zoom_to_cursor_pan(
    pan_after_rescale: Vec2,
    cursor: Vec2,
    size: Vec2,
    old_scale: f32,
    new_scale: f32,
) -> Vec2 {
    if old_scale <= 0.0 {
        return pan_after_rescale;
    }
    // The reference works in its GL frame (y up); convert the cursor offset.
    let offset_x = cursor.x - size.x / 2.0;
    let offset_y = (size.y - cursor.y) - size.y / 2.0;
    let factor = new_scale / old_scale;
    Vec2::new(
        pan_after_rescale.x - (offset_x * factor - offset_x),
        pan_after_rescale.y - (offset_y * factor - offset_y),
    )
}

/// One auto-centre step: ease the pan back toward zero with an exponential
/// approach (time-constant feel of the reference's 0.1 interpolant), snapping
/// to exactly zero once both components are within half a pixel.
pub(crate) fn auto_center_step(pan: Vec2, dt: f32) -> Vec2 {
    let t = 1.0 - (-dt / 0.075).exp();
    let eased = Vec2::new(pan.x * (1.0 - t), pan.y * (1.0 - t));
    if eased.x.abs() < 0.5 && eased.y.abs() < 0.5 {
        Vec2::ZERO
    } else {
        eased
    }
}

// ---------------------------------------------------------------------------
// Layer raster sizing.
// ---------------------------------------------------------------------------

/// The side of the square power-of-two raster backing a cached content layer,
/// derived from the surface diagonal: the least power of two whose double still
/// falls short of the diagonal, clamped to 64–512 (the reference's
/// `createImage`).
pub(crate) fn layer_raster_size(surface: Vec2) -> u32 {
    let diagonal = surface.length();
    let mut size: u32 = 64;
    while let Some(doubled) = size.checked_mul(2) {
        if f32::from(u16::try_from(doubled).unwrap_or(u16::MAX)) < diagonal && size < 512 {
            size = doubled;
        } else {
            break;
        }
    }
    size
}

/// Layer texels per metre: the raster spans the surface diagonal's worth of
/// world, so `texels = raster / (diagonal / scale × 256)` (the reference's
/// `mObjectMapTPM`).
pub(crate) fn layer_texels_per_metre(raster_size: u32, surface: Vec2, scale: f32) -> f32 {
    let diagonal = surface.length();
    if diagonal <= 0.0 || scale <= 0.0 {
        return 1.0;
    }
    let metres = diagonal / scale * REGION_WIDTH_METRES;
    f32::from(u16::try_from(raster_size).unwrap_or(u16::MAX)) / metres
}

// ---------------------------------------------------------------------------
// Raster primitives (bottom-up rows, RGBA8).
// ---------------------------------------------------------------------------

/// A square, bottom-up (row 0 = south) RGBA raster a content layer draws into.
#[derive(Debug, Clone, Default)]
pub(crate) struct LayerRaster {
    /// The square side, in texels.
    pub(crate) size: u32,
    /// RGBA8 texels, row-major from the south row up.
    pub(crate) data: Vec<u8>,
}

impl LayerRaster {
    /// A transparent raster of `size` × `size` texels.
    pub(crate) fn new(size: u32) -> Self {
        let texels = usize::try_from(size).unwrap_or(0);
        Self {
            size,
            data: vec![0; texels.saturating_mul(texels).saturating_mul(4)],
        }
    }

    /// Clear every texel back to transparent, keeping the allocation.
    pub(crate) fn clear(&mut self) {
        self.data.fill(0);
    }

    /// The byte offset of texel (`x`, `y`), or `None` when out of bounds.
    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        let size = i32::try_from(self.size).ok()?;
        if x < 0 || y < 0 || x >= size || y >= size {
            return None;
        }
        let xu = usize::try_from(x).ok()?;
        let yu = usize::try_from(y).ok()?;
        let su = usize::try_from(self.size).ok()?;
        Some(yu.saturating_mul(su).saturating_add(xu).saturating_mul(4))
    }

    /// Write one texel (no blending), ignoring out-of-bounds writes.
    pub(crate) fn put(&mut self, x: i32, y: i32, color: Rgba) {
        if let Some(offset) = self.offset(x, y)
            && let Some(slot) = self.data.get_mut(offset..offset.saturating_add(4))
        {
            slot.copy_from_slice(&color);
        }
    }

    /// Read one texel, or transparent when out of bounds.
    pub(crate) fn get(&self, x: i32, y: i32) -> Rgba {
        let Some(offset) = self.offset(x, y) else {
            return [0, 0, 0, 0];
        };
        match self.data.get(offset..offset.saturating_add(4)) {
            Some(slice) => {
                let mut out = [0u8; 4];
                out.copy_from_slice(slice);
                out
            }
            None => [0, 0, 0, 0],
        }
    }
}

/// Round a finite `f32` to the nearest `i32`, clamped to the `i32` range.
pub(crate) const fn round_i32(value: f32) -> i32 {
    let clamped = value
        .round()
        .clamp(-2_147_483_000.0_f32, 2_147_483_000.0_f32);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "clamped just above to well inside the i32 range, and integral after round()"
    )]
    let out = clamped as i32;
    out
}

/// Draw one map point into a layer raster: a filled square for a level point,
/// or the reference's above-marker (a vertical line capped by a top bar) when
/// `relative_height` is positive — the exact `LLNetMap::renderPoint` shapes.
pub(crate) fn render_point(
    raster: &mut LayerRaster,
    x_offset: i32,
    y_offset: i32,
    color: Rgba,
    diameter: i32,
    relative_height: i32,
) {
    if diameter <= 0 {
        return;
    }
    let size = i32::try_from(raster.size).unwrap_or(0);
    if x_offset < 0 || x_offset >= size || y_offset < 0 || y_offset >= size {
        return;
    }
    let neg_radius = diameter.wrapping_div(2);
    let pos_radius = diameter.saturating_sub(neg_radius);
    let start = neg_radius.saturating_neg();
    if relative_height > 0 {
        // Point above the agent: vertical line plus a top bar.
        for y in start..pos_radius {
            raster.put(x_offset, y_offset.saturating_add(y), color);
        }
        let top = y_offset.saturating_add(pos_radius).saturating_sub(1);
        for x in start..pos_radius {
            raster.put(x_offset.saturating_add(x), top, color);
        }
    } else {
        for x in start..pos_radius {
            for y in start..pos_radius {
                raster.put(
                    x_offset.saturating_add(x),
                    y_offset.saturating_add(y),
                    color,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Object layer.
// ---------------------------------------------------------------------------

/// `PrimFlags` bit: the object is physical (`FLAGS_USE_PHYSICS`).
pub(crate) const FLAG_USE_PHYSICS: u32 = 1 << 0;

/// `PrimFlags` bit: the viewer's agent owns the object
/// (`FLAGS_OBJECT_YOU_OWNER`).
pub(crate) const FLAG_YOU_OWNER: u32 = 1 << 5;

/// `PrimFlags` bit: the object contains a running script (`FLAGS_SCRIPTED`).
pub(crate) const FLAG_SCRIPTED: u32 = 1 << 6;

/// `PrimFlags` bit: the object is phantom (`FLAGS_PHANTOM`).
pub(crate) const FLAG_PHANTOM: u32 = 1 << 10;

/// `PrimFlags` bit: the object is group-owned (`FLAGS_OBJECT_GROUP_OWNED`).
pub(crate) const FLAG_GROUP_OWNED: u32 = 1 << 18;

/// `PrimFlags` bit: the object is temporary-on-rez (`FLAGS_TEMPORARY_ON_REZ`).
pub(crate) const FLAG_TEMP_ON_REZ: u32 = 1 << 29;

/// The scale magnitude above which an unowned object still joins the map
/// layer (the reference's "large object" membership rule).
pub(crate) const LARGE_OBJECT_SCALE: f32 = 7.5;

/// Others' objects above water: dark grey (`NetMapOtherOwnAboveWater`).
pub(crate) const COLOR_OTHER_ABOVE: Rgba = [61, 61, 61, 255];

/// Others' objects below water: darker grey (`NetMapOtherOwnBelowWater`).
pub(crate) const COLOR_OTHER_BELOW: Rgba = [32, 32, 32, 255];

/// Your objects above water: cyan (`NetMapYouOwnAboveWater`).
pub(crate) const COLOR_YOU_ABOVE: Rgba = [0, 255, 255, 255];

/// Your objects below water: darker cyan (`NetMapYouOwnBelowWater`).
pub(crate) const COLOR_YOU_BELOW: Rgba = [0, 199, 199, 255];

/// Group-owned objects above water: magenta (`NetMapGroupOwnAboveWater`).
pub(crate) const COLOR_GROUP_ABOVE: Rgba = [255, 0, 255, 255];

/// Group-owned objects below water: darker magenta
/// (`NetMapGroupOwnBelowWater`).
pub(crate) const COLOR_GROUP_BELOW: Rgba = [199, 0, 199, 255];

/// Scripted-object accent: orange (`NetMapScripted`).
pub(crate) const COLOR_SCRIPTED: Rgba = [255, 165, 0, 255];

/// Temp-on-rez accent: a deeper orange (`NetMapTempOnRez`).
pub(crate) const COLOR_TEMP_ON_REZ: Rgba = [255, 128, 0, 255];

/// Physical accent for your own objects: red (`NetMapYouPhysical`).
pub(crate) const COLOR_YOU_PHYSICAL: Rgba = [255, 0, 0, 255];

/// Physical accent for group-owned objects: green (`NetMapGroupPhysical`).
pub(crate) const COLOR_GROUP_PHYSICAL: Rgba = [0, 255, 0, 255];

/// Physical accent for others' objects: green (`NetMapOtherPhysical`).
pub(crate) const COLOR_OTHER_PHYSICAL: Rgba = [0, 255, 0, 255];

/// The optional accent classes and their master toggles, as read from the
/// settings at layer-regeneration time.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ObjectAccents {
    /// Highlight physical objects (`NetMapPhysical`).
    pub(crate) physical: bool,
    /// Highlight scripted objects (`NetMapScripted`).
    pub(crate) scripted: bool,
    /// Highlight temp-on-rez objects (`NetMapTempOnRez`).
    pub(crate) temp_on_rez: bool,
    /// Phantom-object dot opacity, `0..=255` (`NetMapPhantomOpacity`).
    pub(crate) phantom_alpha: u8,
}

/// Whether an object belongs on the map layer at all: owned by you, large, or
/// carrying one of the enabled accent classes — never an attachment (the
/// caller excludes those before asking).
pub(crate) fn object_on_map(flags: u32, scale: [f32; 3], accents: ObjectAccents) -> bool {
    if flags & FLAG_YOU_OWNER != 0 {
        return true;
    }
    let [sx, sy, sz] = scale;
    if (sx * sx + sy * sy + sz * sz).sqrt() > LARGE_OBJECT_SCALE {
        return true;
    }
    (accents.physical && flags & FLAG_USE_PHYSICS != 0)
        || (accents.scripted && flags & FLAG_SCRIPTED != 0)
        || (accents.temp_on_rez && flags & FLAG_TEMP_ON_REZ != 0)
}

/// The dot radius (metres) an object rasterises at: the reference's
/// `(scale.x + scale.y) × 0.25 × 1.3` fudge, clamped to `max_radius`, with a
/// 2 m floor for owned / accented objects so your small things stay visible.
pub(crate) fn object_map_radius(
    scale: [f32; 3],
    flags: u32,
    accents: ObjectAccents,
    max_radius: f32,
) -> f32 {
    let [sx, sy, _sz] = scale;
    let mut radius = (sx + sy) * 0.25 * 1.3;
    radius = radius.min(max_radius);
    let accented = (accents.physical && flags & FLAG_USE_PHYSICS != 0)
        || (accents.scripted && flags & FLAG_SCRIPTED != 0)
        || (accents.temp_on_rez && flags & FLAG_TEMP_ON_REZ != 0);
    if (flags & FLAG_YOU_OWNER != 0 || accented) && radius < 2.0 {
        radius = 2.0;
    }
    radius
}

/// The colour an object rasterises in: ownership class × above/below the
/// region water height, with the enabled accent classes overriding in the
/// reference's order (scripted, then physical, then temp-on-rez) and phantom
/// objects taking the phantom opacity.
pub(crate) const fn object_map_color(
    flags: u32,
    above_water: bool,
    accents: ObjectAccents,
) -> Rgba {
    let mut color = if flags & FLAG_YOU_OWNER != 0 {
        match (flags & FLAG_GROUP_OWNED != 0, above_water) {
            (true, true) => COLOR_GROUP_ABOVE,
            (true, false) => COLOR_GROUP_BELOW,
            (false, true) => COLOR_YOU_ABOVE,
            (false, false) => COLOR_YOU_BELOW,
        }
    } else if above_water {
        COLOR_OTHER_ABOVE
    } else {
        COLOR_OTHER_BELOW
    };
    if accents.scripted && flags & FLAG_SCRIPTED != 0 {
        color = COLOR_SCRIPTED;
    }
    if accents.physical && flags & FLAG_USE_PHYSICS != 0 {
        color = if flags & FLAG_YOU_OWNER != 0 {
            if flags & FLAG_GROUP_OWNED != 0 {
                COLOR_GROUP_PHYSICAL
            } else {
                COLOR_YOU_PHYSICAL
            }
        } else {
            COLOR_OTHER_PHYSICAL
        };
    }
    if accents.temp_on_rez && flags & FLAG_TEMP_ON_REZ != 0 {
        color = COLOR_TEMP_ON_REZ;
    }
    if flags & FLAG_PHANTOM != 0 {
        color = [color[0], color[1], color[2], accents.phantom_alpha];
    }
    color
}

/// Rasterise one object into the layer: `east` / `north` are metres relative
/// to the layer's capture centre, `radius_metres` the [`object_map_radius`].
pub(crate) fn render_object_point(
    raster: &mut LayerRaster,
    texels_per_metre: f32,
    east: f32,
    north: f32,
    color: Rgba,
    radius_metres: f32,
) {
    let half = f32::from(u16::try_from(raster.size).unwrap_or(u16::MAX)) / 2.0;
    let x_offset = round_i32(east * texels_per_metre + half);
    let y_offset = round_i32(north * texels_per_metre + half);
    let diameter = round_i32(2.0 * radius_metres * texels_per_metre);
    render_point(raster, x_offset, y_offset, color, diameter, 0);
}

// ---------------------------------------------------------------------------
// Parcel layer.
// ---------------------------------------------------------------------------

/// The parcel property-line colour (`MapParcelOutlineColor`).
pub(crate) const COLOR_PARCEL_LINE: Rgba = [255, 255, 255, 255];

/// The for-sale parcel fill: pale yellow.
pub(crate) const COLOR_FOR_SALE: Rgba = [255, 255, 128, 192];

/// The auction parcel fill: violet.
pub(crate) const COLOR_AUCTION: Rgba = [128, 0, 255, 102];

/// The size of one parcel-overlay cell in metres.
pub(crate) const PARCEL_CELL_METRES: f32 = 4.0;

/// A parcel-overlay cell's fill class on the minimap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParcelFill {
    /// The cell is for sale: the pale-yellow fill.
    ForSale,
    /// The cell is up for auction: the violet fill.
    Auction,
}

/// One parcel-overlay cell as the parcel-layer rasteriser consumes it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ParcelCell {
    /// The cell's fill (for sale / auction), when it has one.
    pub(crate) fill: Option<ParcelFill>,
    /// The cell's west edge is a property line.
    pub(crate) west_line: bool,
    /// The cell's south edge is a property line.
    pub(crate) south_line: bool,
}

/// Draw one region's parcel overlay into the parcel layer raster: the region's
/// north / east border lines, the per-cell for-sale / auction fills (when
/// enabled) and the south / west property lines — the reference's
/// `renderPropertyLinesForRegion`.
///
/// `origin_east` / `origin_north` are the region's south-west corner in metres
/// relative to the layer capture centre; `cell(row, col)` yields the decoded
/// overlay cell (row = south→north, col = west→east) or `None` outside the
/// grid; `grids_per_edge` is the overlay's cell count per edge. With
/// `full_border` the south row and west column are drawn too — for a region
/// whose overlay is unknown (a neighbour), whose edge cells cannot supply its
/// south / west property lines the way the reference's per-region overlays
/// do.
#[expect(
    clippy::too_many_arguments,
    reason = "a direct port of the reference rasteriser, which takes exactly this data; bundling \
              into a one-use struct would only rename the arguments"
)]
pub(crate) fn render_parcel_region(
    raster: &mut LayerRaster,
    texels_per_metre: f32,
    origin_east: f32,
    origin_north: f32,
    region_width: f32,
    line_color: Rgba,
    show_for_sale: bool,
    full_border: bool,
    grids_per_edge: usize,
    cell: &dyn Fn(usize, usize) -> Option<ParcelCell>,
) {
    let half = f32::from(u16::try_from(raster.size).unwrap_or(u16::MAX)) / 2.0;
    let origin_x = round_i32(origin_east * texels_per_metre + half);
    let origin_y = round_i32(origin_north * texels_per_metre + half);
    let size = i32::try_from(raster.size).unwrap_or(0);
    let width_texels = round_i32(region_width * texels_per_metre);

    // North border row and east border column.
    let border_y = origin_y.saturating_add(width_texels);
    if border_y >= 0 && border_y < size {
        for x in origin_x.max(0)
            ..=origin_x
                .saturating_add(width_texels)
                .min(size.saturating_sub(1))
        {
            raster.put(x, border_y, line_color);
        }
    }
    let border_x = origin_x.saturating_add(width_texels);
    if border_x >= 0 && border_x < size {
        for y in origin_y.max(0)
            ..=origin_y
                .saturating_add(width_texels)
                .min(size.saturating_sub(1))
        {
            raster.put(border_x, y, line_color);
        }
    }
    if full_border {
        if origin_y >= 0 && origin_y < size {
            for x in origin_x.max(0)
                ..=origin_x
                    .saturating_add(width_texels)
                    .min(size.saturating_sub(1))
            {
                raster.put(x, origin_y, line_color);
            }
        }
        if origin_x >= 0 && origin_x < size {
            for y in origin_y.max(0)
                ..=origin_y
                    .saturating_add(width_texels)
                    .min(size.saturating_sub(1))
            {
                raster.put(origin_x, y, line_color);
            }
        }
    }

    let step_texels = PARCEL_CELL_METRES * texels_per_metre;
    for row in 0..grids_per_edge {
        for col in 0..grids_per_edge {
            let Some(cell_data) = cell(row, col) else {
                continue;
            };
            if cell_data.fill.is_none() && !cell_data.west_line && !cell_data.south_line {
                continue;
            }
            let col_f = f32::from(u16::try_from(col).unwrap_or(u16::MAX));
            let row_f = f32::from(u16::try_from(row).unwrap_or(u16::MAX));
            let pos_x = origin_x.saturating_add(round_i32(col_f * step_texels));
            let pos_y = origin_y.saturating_add(round_i32(row_f * step_texels));
            let span = round_i32(step_texels);

            if show_for_sale && let Some(fill_class) = cell_data.fill {
                let fill = match fill_class {
                    ParcelFill::ForSale => COLOR_FOR_SALE,
                    ParcelFill::Auction => COLOR_AUCTION,
                };
                for y in pos_y.max(0)..=pos_y.saturating_add(span).min(size.saturating_sub(1)) {
                    for x in pos_x.max(0)..=pos_x.saturating_add(span).min(size.saturating_sub(1)) {
                        raster.put(x, y, fill);
                    }
                }
            }
            if cell_data.south_line && pos_y >= 0 && pos_y < size {
                for x in pos_x.max(0)..=pos_x.saturating_add(span).min(size.saturating_sub(1)) {
                    raster.put(x, pos_y, line_color);
                }
            }
            if cell_data.west_line && pos_x >= 0 && pos_x < size {
                for y in pos_y.max(0)..=pos_y.saturating_add(span).min(size.saturating_sub(1)) {
                    raster.put(pos_x, y, line_color);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Compass labels.
// ---------------------------------------------------------------------------

/// Where a compass label sits: its centre position relative to the map centre,
/// in surface pixels (top-left-origin frame, `y` down).
///
/// `angle` is the label's world direction rotated by the map rotation (`0` =
/// toward the right edge, counter-clockwise positive in the GL frame — the
/// caller passes `map_rotation` for East, `map_rotation + π/2` for North and
/// so on); `half_width` / `half_height` are the usable half-extents (map half
/// size minus label half size minus padding). The label is projected onto the
/// rect edge along its angle — the reference's `setDirectionPos`.
pub(crate) fn compass_label_offset(angle: f32, half_width: f32, half_height: f32) -> Vec2 {
    let corner_angle = half_height.atan2(half_width);
    // Mirror the angle into the upper-right quadrant to pick the edge.
    let normalized = angle.rem_euclid(core::f32::consts::TAU);
    let into_top = if normalized > core::f32::consts::PI {
        core::f32::consts::TAU - normalized
    } else {
        normalized
    };
    let into_top_right =
        core::f32::consts::FRAC_PI_2 - (into_top - core::f32::consts::FRAC_PI_2).abs();
    let at_side_edge = into_top_right < corner_angle;

    let part_x = angle.cos();
    let part_y = angle.sin();
    let (x, y) = if at_side_edge {
        let x = half_width.copysign(part_x);
        let y = if part_x.abs() > f32::EPSILON {
            x * part_y / part_x
        } else {
            0.0
        };
        (x, y)
    } else {
        let y = half_height.copysign(part_y);
        let x = if part_y.abs() > f32::EPSILON {
            y * part_x / part_y
        } else {
            0.0
        };
        (x, y)
    };
    // GL frame (y up) → surface frame (y down).
    Vec2::new(x, -y)
}

/// Whether the diagonal compass labels are shown: hidden once a label's box
/// would cover more than [`MINOR_DIR_THRESHOLD`] of the map's smaller edge.
pub(crate) fn minor_directions_visible(label_height: f32, surface: Vec2) -> bool {
    label_height < MINOR_DIR_THRESHOLD * surface.x.min(surface.y)
}

// ---------------------------------------------------------------------------
// Avatar dots.
// ---------------------------------------------------------------------------

/// The base avatar dot colour (`MapAvatarColor`): red.
pub(crate) const COLOR_AVATAR: Rgba = [255, 0, 0, 255];

/// The friend dot colour (`MapAvatarFriendColor`): green.
pub(crate) const COLOR_AVATAR_FRIEND: Rgba = [0, 255, 0, 255];

/// The self marker colour (`MapAvatarSelfColor`): yellow.
pub(crate) const COLOR_AVATAR_SELF: Rgba = [255, 255, 0, 255];

/// The Linden dot colour (`MapAvatarLindenColor`): blue.
pub(crate) const COLOR_AVATAR_LINDEN: Rgba = [0, 0, 255, 255];

/// The tracking-beacon colour (`MapTrackColor`): red.
pub(crate) const COLOR_TRACK: Rgba = [255, 0, 0, 255];

/// The camera frustum wedge colour: white at 0.1 alpha (`MapFrustumColor`).
pub(crate) const COLOR_FRUSTUM: Rgba = [255, 255, 255, 26];

/// The whisper-range ring colour: blue at 0.3 alpha (`MapWhisperRingColor`).
pub(crate) const COLOR_WHISPER_RING: Rgba = [0, 0, 255, 77];

/// The chat-range ring colour: yellow at 0.3 alpha (`MapChatRingColor`).
pub(crate) const COLOR_CHAT_RING: Rgba = [255, 255, 0, 77];

/// The shout-range ring colour: red at 0.3 alpha (`MapShoutRingColor`).
pub(crate) const COLOR_SHOUT_RING: Rgba = [255, 0, 0, 77];

/// The height-cue glyph an avatar dot draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeightGlyph {
    /// Within ±[`HEIGHT_CUE_BAND`] of the camera: a level dot.
    Level,
    /// More than the band above the camera: an up chevron.
    Above,
    /// More than the band below the camera: a down chevron.
    Below,
    /// Altitude unknown (coarse sentinel with the camera itself high up).
    Unknown,
}

/// Select the height-cue glyph from the avatar's altitude relative to the
/// camera. `altitude_unknown` marks the coarse-location sentinel; the
/// reference draws *unknown* only when the camera is itself at or above the
/// coarse ceiling, and otherwise treats the avatar as far above.
pub(crate) fn height_glyph(relative_z: f32, altitude_unknown: bool, camera_z: f32) -> HeightGlyph {
    if altitude_unknown {
        if camera_z >= COARSE_MAX_Z {
            return HeightGlyph::Unknown;
        }
        return HeightGlyph::Above;
    }
    if relative_z > HEIGHT_CUE_BAND {
        HeightGlyph::Above
    } else if relative_z < -HEIGHT_CUE_BAND {
        HeightGlyph::Below
    } else {
        HeightGlyph::Level
    }
}

/// Whether a coarse-location altitude is the "unknown" sentinel: the encoded
/// byte saturates at 255 (1020 m) and some simulators send 0 for unknown.
pub(crate) fn coarse_altitude_unknown(z_metres: f32) -> bool {
    z_metres <= 0.0 || z_metres >= COARSE_MAX_Z
}

/// The avatar dot radius in surface pixels at the given pixels-per-metre.
pub(crate) fn dot_radius(pixels_per_metre: f32) -> f32 {
    (DOT_SCALE * pixels_per_metre).max(MIN_DOT_RADIUS)
}

// ---------------------------------------------------------------------------
// Surface overlay painters (top-left-origin RGBA surface).
// ---------------------------------------------------------------------------

/// A mutable view of the composited surface image (row 0 = top).
pub(crate) struct Surface<'a> {
    /// The surface width in pixels.
    pub(crate) width: u32,
    /// The surface height in pixels.
    pub(crate) height: u32,
    /// RGBA8 pixels, row-major from the top row down.
    pub(crate) data: &'a mut [u8],
}

impl Surface<'_> {
    /// The byte offset of pixel (`x`, `y`), or `None` when out of bounds.
    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        let width = i32::try_from(self.width).ok()?;
        let height = i32::try_from(self.height).ok()?;
        if x < 0 || y < 0 || x >= width || y >= height {
            return None;
        }
        let xu = usize::try_from(x).ok()?;
        let yu = usize::try_from(y).ok()?;
        let wu = usize::try_from(self.width).ok()?;
        Some(yu.saturating_mul(wu).saturating_add(xu).saturating_mul(4))
    }

    /// Alpha-blend `color` over pixel (`x`, `y`), ignoring out-of-bounds.
    pub(crate) fn blend(&mut self, x: i32, y: i32, color: Rgba) {
        let Some(offset) = self.offset(x, y) else {
            return;
        };
        let Some(slot) = self.data.get_mut(offset..offset.saturating_add(4)) else {
            return;
        };
        let mut dest = [0u8; 4];
        dest.copy_from_slice(slot);
        slot.copy_from_slice(&blend_over(dest, color));
    }
}

/// Straight-alpha "source over destination" blending, returning an opaque-ish
/// result (destination alpha saturates upward).
pub(crate) fn blend_over(dest: Rgba, src: Rgba) -> Rgba {
    let sa = f32::from(src[3]) / 255.0;
    let da = f32::from(dest[3]) / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let channel = |s: u8, d: u8| -> u8 {
        let value = (f32::from(s) * sa + f32::from(d) * da * (1.0 - sa)) / out_a;
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the blend of two 0..=255 channels at 0..=1 weights stays in 0..=255"
        )]
        let out = value.clamp(0.0, 255.0).round() as u8;
        out
    };
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an alpha composite of two 0..=1 alphas scaled back to 0..=255 stays in range"
    )]
    let alpha = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
    [
        channel(src[0], dest[0]),
        channel(src[1], dest[1]),
        channel(src[2], dest[2]),
        alpha,
    ]
}

/// Fill a translucent circular wedge (the camera frustum) centred at
/// (`cx`, `cy`) on the surface: radius `radius` pixels, aimed along
/// `direction` (radians, `0` = up, counter-clockwise positive in the GL frame)
/// with total angular width `width` radians.
pub(crate) fn draw_wedge(
    surface: &mut Surface<'_>,
    cx: f32,
    cy: f32,
    radius: f32,
    direction: f32,
    width: f32,
    color: Rgba,
) {
    if radius <= 0.0 || width <= 0.0 {
        return;
    }
    let min_x = round_i32(cx - radius);
    let max_x = round_i32(cx + radius);
    let min_y = round_i32(cy - radius);
    let max_y = round_i32(cy + radius);
    let half_width = width / 2.0;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let x_f = i32_to_f32(x);
            let y_f = i32_to_f32(y);
            let dx = x_f + 0.5 - cx;
            // Surface y grows downward; the GL-frame "up" is -dy.
            let dy = cy - (y_f + 0.5);
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > radius {
                continue;
            }
            // Angle of the pixel from "up", counter-clockwise positive.
            let angle = (-dx).atan2(dy);
            let mut delta = angle - direction;
            while delta > core::f32::consts::PI {
                delta -= core::f32::consts::TAU;
            }
            while delta < -core::f32::consts::PI {
                delta += core::f32::consts::TAU;
            }
            if delta.abs() <= half_width {
                surface.blend(x, y, color);
            }
        }
    }
}

/// Stroke a circle (a chat-range ring) of `radius` pixels and `thickness`
/// pixels centred at (`cx`, `cy`).
pub(crate) fn draw_ring(
    surface: &mut Surface<'_>,
    cx: f32,
    cy: f32,
    radius: f32,
    thickness: f32,
    color: Rgba,
) {
    if radius <= 0.0 {
        return;
    }
    let reach = radius + thickness;
    let min_x = round_i32(cx - reach);
    let max_x = round_i32(cx + reach);
    let min_y = round_i32(cy - reach);
    let max_y = round_i32(cy + reach);
    let half = thickness / 2.0;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = i32_to_f32(x) + 0.5 - cx;
            let dy = i32_to_f32(y) + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if (dist - radius).abs() <= half {
                surface.blend(x, y, color);
            }
        }
    }
}

/// Fill a solid disc of `radius` pixels centred at (`cx`, `cy`).
pub(crate) fn draw_disc(surface: &mut Surface<'_>, cx: f32, cy: f32, radius: f32, color: Rgba) {
    let min_x = round_i32(cx - radius);
    let max_x = round_i32(cx + radius);
    let min_y = round_i32(cy - radius);
    let max_y = round_i32(cy + radius);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = i32_to_f32(x) + 0.5 - cx;
            let dy = i32_to_f32(y) + 0.5 - cy;
            if (dx * dx + dy * dy).sqrt() <= radius {
                surface.blend(x, y, color);
            }
        }
    }
}

/// Draw one avatar glyph at surface position (`cx`, `cy`): a disc for a level
/// avatar, an up- / down-pointing chevron triangle for above / below, and a
/// hollow ring for unknown altitude.
pub(crate) fn draw_avatar_glyph(
    surface: &mut Surface<'_>,
    cx: f32,
    cy: f32,
    radius: f32,
    glyph: HeightGlyph,
    color: Rgba,
) {
    match glyph {
        HeightGlyph::Level => draw_disc(surface, cx, cy, radius, color),
        HeightGlyph::Unknown => draw_ring(surface, cx, cy, radius, 2.0, color),
        HeightGlyph::Above | HeightGlyph::Below => {
            // A filled triangle pointing up (screen -y) or down.
            let up = matches!(glyph, HeightGlyph::Above);
            let min_y = round_i32(cy - radius);
            let max_y = round_i32(cy + radius);
            for y in min_y..=max_y {
                // 0 at the tip row, 1 at the base row.
                let along = if up {
                    (i32_to_f32(y) + 0.5 - (cy - radius)) / (2.0 * radius)
                } else {
                    ((cy + radius) - (i32_to_f32(y) + 0.5)) / (2.0 * radius)
                };
                if !(0.0..=1.0).contains(&along) {
                    continue;
                }
                let half_span = along * radius;
                let min_x = round_i32(cx - half_span);
                let max_x = round_i32(cx + half_span);
                for x in min_x..=max_x {
                    surface.blend(x, y, color);
                }
            }
        }
    }
}

/// Draw the tracking beacon: a dot when on the surface, otherwise a small
/// triangle on the surface edge pointing toward the target.
pub(crate) fn draw_tracking(surface: &mut Surface<'_>, position: Vec2, color: Rgba) {
    let width = u32_to_f32(surface.width);
    let height = u32_to_f32(surface.height);
    let on_surface =
        position.x >= 0.0 && position.y >= 0.0 && position.x < width && position.y < height;
    if on_surface {
        draw_disc(surface, position.x, position.y, 4.0, color);
        return;
    }
    // Clamp to the edge and draw an arrow-ish triangle pointing outward.
    let cx = position.x.clamp(4.0, width - 4.0);
    let cy = position.y.clamp(4.0, height - 4.0);
    let dir_x = position.x - cx;
    let dir_y = position.y - cy;
    let length = (dir_x * dir_x + dir_y * dir_y).sqrt().max(1.0);
    let ux = dir_x / length;
    let uy = dir_y / length;
    // Perpendicular for the triangle base.
    let px = -uy;
    let py = ux;
    let tip = (cx + ux * 5.0, cy + uy * 5.0);
    let base_a = (cx - ux * 3.0 + px * 4.0, cy - uy * 3.0 + py * 4.0);
    let base_b = (cx - ux * 3.0 - px * 4.0, cy - uy * 3.0 - py * 4.0);
    fill_triangle(surface, tip, base_a, base_b, color);
}

/// Fill a triangle given by three surface points, by sign-of-edge testing over
/// the bounding box (small triangles only — the tracking arrow).
fn fill_triangle(
    surface: &mut Surface<'_>,
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    color: Rgba,
) {
    let min_x = round_i32(a.0.min(b.0).min(c.0));
    let max_x = round_i32(a.0.max(b.0).max(c.0));
    let min_y = round_i32(a.1.min(b.1).min(c.1));
    let max_y = round_i32(a.1.max(b.1).max(c.1));
    let edge = |p: (f32, f32), q: (f32, f32), x: f32, y: f32| -> f32 {
        (q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0)
    };
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let fx = i32_to_f32(x) + 0.5;
            let fy = i32_to_f32(y) + 0.5;
            let e0 = edge(a, b, fx, fy);
            let e1 = edge(b, c, fx, fy);
            let e2 = edge(c, a, fx, fy);
            if (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0) {
                surface.blend(x, y, color);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Terrain shading.
// ---------------------------------------------------------------------------

/// Approximate archetype colours for the four terrain detail slots (dirt,
/// grass, mountain, rock) — a legible stand-in for compositing the real detail
/// textures (which is a follow-up; the reference composites the sim surface
/// texture on Second Life and world-map tiles on OpenSim).
pub(crate) const TERRAIN_LAYER_COLORS: [[f32; 3]; 4] = [
    [0.45, 0.36, 0.26],
    [0.30, 0.42, 0.22],
    [0.42, 0.38, 0.34],
    [0.53, 0.53, 0.53],
];

/// The deep-water tint terrain fades toward below the water height.
pub(crate) const TERRAIN_WATER_COLOR: [f32; 3] = [0.10, 0.22, 0.35];

/// Shade one terrain texel: blend the four archetype colours by `weights`,
/// apply a simple slope-based light (gradient toward the north-west light),
/// and fade toward the water tint below `water_height`.
pub(crate) fn terrain_texel_color(
    height: f32,
    weights: [f32; 4],
    gradient_east: f32,
    gradient_north: f32,
    water_height: f32,
) -> Rgba {
    let mut r = 0.0;
    let mut g = 0.0;
    let mut b = 0.0;
    for (weight, color) in weights.iter().zip(TERRAIN_LAYER_COLORS.iter()) {
        r += weight * color[0];
        g += weight * color[1];
        b += weight * color[2];
    }
    // Hillshade: light from the north-west, strength from the slope.
    let shade = (1.0 + 0.35 * (-gradient_east + gradient_north) / 2.0).clamp(0.55, 1.35);
    r *= shade;
    g *= shade;
    b *= shade;
    if height < water_height {
        let depth = (water_height - height).min(8.0) / 8.0;
        let mix = 0.35 + 0.65 * depth;
        r = r * (1.0 - mix) + TERRAIN_WATER_COLOR[0] * mix;
        g = g * (1.0 - mix) + TERRAIN_WATER_COLOR[1] * mix;
        b = b * (1.0 - mix) + TERRAIN_WATER_COLOR[2] * mix;
    }
    [unit_byte(r), unit_byte(g), unit_byte(b), 255]
}

/// Quantise a `0..=1` channel to a byte.
fn unit_byte(value: f32) -> u8 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0.0..=1.0 and scaled to 0..=255 before the cast"
    )]
    let out = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    out
}

/// Widen a small (≤ `u16::MAX`) pixel count to `f32`.
pub(crate) fn u32_to_f32(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

/// Widen a small (± `i16` range) pixel coordinate to `f32`.
pub(crate) fn i32_to_f32(value: i32) -> f32 {
    f32::from(
        i16::try_from(value.clamp(i32::from(i16::MIN), i32::from(i16::MAX))).unwrap_or(i16::MAX),
    )
}

// ---------------------------------------------------------------------------
// Double-click action.
// ---------------------------------------------------------------------------

/// What double-clicking the map does (`NetMapDoubleClickAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoubleClickAction {
    /// Do nothing.
    Nothing,
    /// Open the world map (unlanded today — falls back to nothing).
    WorldMap,
    /// Teleport to the clicked point.
    Teleport,
}

impl DoubleClickAction {
    /// Decode the persisted integer setting (`0` none, `1` world map,
    /// `2` teleport — the default), treating anything else as nothing.
    pub(crate) const fn from_setting(value: i32) -> Self {
        match value {
            1 => Self::WorldMap,
            2 => Self::Teleport,
            _ => Self::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COARSE_MAX_Z, COLOR_AUCTION, COLOR_FOR_SALE, COLOR_GROUP_ABOVE, COLOR_OTHER_ABOVE,
        COLOR_OTHER_BELOW, COLOR_PARCEL_LINE, COLOR_SCRIPTED, COLOR_TEMP_ON_REZ, COLOR_YOU_ABOVE,
        COLOR_YOU_BELOW, DoubleClickAction, FLAG_GROUP_OWNED, FLAG_SCRIPTED, FLAG_TEMP_ON_REZ,
        FLAG_YOU_OWNER, HeightGlyph, LayerRaster, MAP_SCALE_MAX, MAP_SCALE_MEDIUM, MAP_SCALE_MIN,
        MapView, ObjectAccents, ParcelCell, ParcelFill, Surface, auto_center_step, clamp_scale,
        coarse_altitude_unknown, compass_label_offset, dot_radius, draw_avatar_glyph, height_glyph,
        layer_raster_size, layer_texels_per_metre, minor_directions_visible, object_map_color,
        object_map_radius, object_on_map, render_object_point, render_parcel_region, render_point,
        rescale_pan, rotation_for_camera, wheel_scale, zoom_to_cursor_pan,
    };
    use bevy::math::Vec2;
    use pretty_assertions::assert_eq;

    /// A default 200×200 north-up view at the default scale.
    fn view() -> MapView {
        MapView {
            scale: MAP_SCALE_MEDIUM,
            rotation: 0.0,
            pan: Vec2::ZERO,
            size: Vec2::new(200.0, 200.0),
        }
    }

    #[test]
    fn north_up_transform_places_north_above_and_east_right() {
        let view = view();
        let centre = view.view_from_rel(0.0, 0.0);
        assert!((centre.x - 100.0).abs() < 0.001);
        assert!((centre.y - 100.0).abs() < 0.001);
        // 100 m north at 0.5 px/m: 50 px up the surface (smaller y).
        let north = view.view_from_rel(0.0, 100.0);
        assert!((north.x - 100.0).abs() < 0.001);
        assert!((north.y - 50.0).abs() < 0.001);
        // 100 m east: 50 px to the right.
        let east = view.view_from_rel(100.0, 0.0);
        assert!((east.x - 150.0).abs() < 0.001);
        assert!((east.y - 100.0).abs() < 0.001);
    }

    #[test]
    fn camera_at_top_rotation_turns_the_camera_heading_upward() {
        // Camera looking east: rotation is π/2 and an eastward offset lands at
        // the top of the map.
        let rotation = rotation_for_camera(1.0, 0.0);
        assert!((rotation - core::f32::consts::FRAC_PI_2).abs() < 0.001);
        let view = MapView { rotation, ..view() };
        let ahead = view.view_from_rel(100.0, 0.0);
        assert!((ahead.x - 100.0).abs() < 0.01);
        assert!((ahead.y - 50.0).abs() < 0.01);
    }

    #[test]
    fn transform_round_trips_with_rotation_and_pan() {
        let view = MapView {
            scale: 256.0,
            rotation: 0.7,
            pan: Vec2::new(13.0, -6.0),
            size: Vec2::new(311.0, 177.0),
        };
        let (east, north) = (-37.5, 91.25);
        let surface = view.view_from_rel(east, north);
        let (back_east, back_north) = view.rel_from_view(surface);
        assert!((back_east - east).abs() < 0.01);
        assert!((back_north - north).abs() < 0.01);
    }

    #[test]
    fn scale_clamps_to_the_reference_range() {
        assert!((clamp_scale(1.0) - MAP_SCALE_MIN).abs() < f32::EPSILON);
        assert!((clamp_scale(1_000_000.0) - MAP_SCALE_MAX).abs() < f32::EPSILON);
        assert!((clamp_scale(128.0) - 128.0).abs() < f32::EPSILON);
    }

    #[test]
    fn wheel_zoom_applies_four_percent_per_notch() {
        // One notch toward the user (negative clicks) zooms in by 4 %.
        let zoomed = wheel_scale(128.0, -1.0);
        assert!((zoomed - 128.0 * 1.04).abs() < 0.01);
        let out = wheel_scale(128.0, 1.0);
        assert!((out - 128.0 / 1.04).abs() < 0.01);
    }

    #[test]
    fn pan_rescales_with_the_scale() {
        let pan = rescale_pan(Vec2::new(10.0, -20.0), 128.0, 256.0);
        assert!((pan.x - 20.0).abs() < 0.001);
        assert!((pan.y + 40.0).abs() < 0.001);
    }

    #[test]
    fn zoom_toward_cursor_keeps_the_cursor_point_fixed() {
        let size = Vec2::new(200.0, 200.0);
        let old_scale = 128.0;
        let new_scale = 256.0;
        let cursor = Vec2::new(150.0, 60.0);
        let before = MapView {
            scale: old_scale,
            rotation: 0.0,
            pan: Vec2::ZERO,
            size,
        };
        let (east, north) = before.rel_from_view(cursor);
        let pan = zoom_to_cursor_pan(
            rescale_pan(Vec2::ZERO, old_scale, new_scale),
            cursor,
            size,
            old_scale,
            new_scale,
        );
        let after = MapView {
            scale: new_scale,
            rotation: 0.0,
            pan,
            size,
        };
        let surface = after.view_from_rel(east, north);
        assert!((surface.x - cursor.x).abs() < 0.01);
        assert!((surface.y - cursor.y).abs() < 0.01);
    }

    #[test]
    fn auto_center_eases_toward_zero_and_snaps() {
        let stepped = auto_center_step(Vec2::new(100.0, 0.0), 0.016);
        assert!(stepped.x < 100.0);
        assert!(stepped.x > 0.0);
        assert_eq!(auto_center_step(Vec2::new(0.4, 0.4), 0.016), Vec2::ZERO);
    }

    #[test]
    fn layer_raster_size_is_a_clamped_power_of_two() {
        assert_eq!(layer_raster_size(Vec2::new(10.0, 10.0)), 64);
        assert_eq!(layer_raster_size(Vec2::new(200.0, 200.0)), 256);
        assert_eq!(layer_raster_size(Vec2::new(4000.0, 4000.0)), 512);
    }

    #[test]
    fn layer_texels_per_metre_matches_the_reference_formula() {
        let surface = Vec2::new(200.0, 200.0);
        let raster = layer_raster_size(surface);
        let tpm = layer_texels_per_metre(raster, surface, 128.0);
        // diag ≈ 282.8 px → 565.7 m of world at 0.5 px/m → 256 / 565.7.
        assert!((tpm - 0.452_5).abs() < 0.001);
    }

    #[test]
    fn render_point_level_fills_a_square() {
        let mut raster = LayerRaster::new(16);
        render_point(&mut raster, 8, 8, [255, 0, 0, 255], 4, 0);
        assert_eq!(raster.get(8, 8), [255, 0, 0, 255]);
        assert_eq!(raster.get(6, 6), [255, 0, 0, 255]);
        assert_eq!(raster.get(10, 10), [0, 0, 0, 0]);
    }

    #[test]
    fn render_point_above_draws_line_and_cap() {
        let mut raster = LayerRaster::new(16);
        render_point(&mut raster, 8, 8, [0, 255, 0, 255], 5, 1);
        // The vertical line through the centre column.
        assert_eq!(raster.get(8, 7), [0, 255, 0, 255]);
        assert_eq!(raster.get(8, 9), [0, 255, 0, 255]);
        // The top cap row.
        assert_eq!(raster.get(7, 10), [0, 255, 0, 255]);
        // Not a filled square.
        assert_eq!(raster.get(6, 7), [0, 0, 0, 0]);
    }

    #[test]
    fn object_membership_follows_ownership_and_size() {
        let accents = ObjectAccents::default();
        // Your small object: in.
        assert!(object_on_map(FLAG_YOU_OWNER, [0.5, 0.5, 0.5], accents));
        // Someone else's small object: out.
        assert!(!object_on_map(0, [0.5, 0.5, 0.5], accents));
        // A large unowned object: in.
        assert!(object_on_map(0, [10.0, 3.0, 1.0], accents));
        // A scripted small object joins only with the accent toggle on.
        assert!(!object_on_map(FLAG_SCRIPTED, [0.5, 0.5, 0.5], accents));
        let with_scripted = ObjectAccents {
            scripted: true,
            ..accents
        };
        assert!(object_on_map(FLAG_SCRIPTED, [0.5, 0.5, 0.5], with_scripted));
    }

    #[test]
    fn object_radius_clamps_and_floors() {
        let accents = ObjectAccents::default();
        // A megaprim clamps to the max radius.
        let clamped = object_map_radius([200.0, 200.0, 1.0], 0, accents, 16.0);
        assert!((clamped - 16.0).abs() < f32::EPSILON);
        // Your tiny object floors at 2 m.
        let floored = object_map_radius([0.2, 0.2, 0.2], FLAG_YOU_OWNER, accents, 16.0);
        assert!((floored - 2.0).abs() < f32::EPSILON);
        // Someone else's tiny object keeps its computed radius.
        let tiny = object_map_radius([0.2, 0.2, 0.2], 0, accents, 16.0);
        assert!(tiny < 0.2);
    }

    #[test]
    fn object_colors_follow_ownership_and_water() {
        let accents = ObjectAccents::default();
        assert_eq!(object_map_color(0, true, accents), COLOR_OTHER_ABOVE);
        assert_eq!(object_map_color(0, false, accents), COLOR_OTHER_BELOW);
        assert_eq!(
            object_map_color(FLAG_YOU_OWNER, true, accents),
            COLOR_YOU_ABOVE
        );
        assert_eq!(
            object_map_color(FLAG_YOU_OWNER, false, accents),
            COLOR_YOU_BELOW
        );
        assert_eq!(
            object_map_color(FLAG_YOU_OWNER | FLAG_GROUP_OWNED, true, accents),
            COLOR_GROUP_ABOVE
        );
    }

    #[test]
    fn object_accent_colors_override_in_reference_order() {
        let accents = ObjectAccents {
            scripted: true,
            temp_on_rez: true,
            ..ObjectAccents::default()
        };
        assert_eq!(
            object_map_color(FLAG_SCRIPTED, true, accents),
            COLOR_SCRIPTED
        );
        // Temp-on-rez wins over scripted (applied later, as in the reference).
        assert_eq!(
            object_map_color(FLAG_SCRIPTED | FLAG_TEMP_ON_REZ, true, accents),
            COLOR_TEMP_ON_REZ
        );
    }

    #[test]
    fn object_point_rasterises_relative_to_the_layer_centre() {
        let mut raster = LayerRaster::new(64);
        render_object_point(&mut raster, 1.0, 10.0, -5.0, COLOR_YOU_ABOVE, 2.0);
        // 10 m east, 5 m south of centre at 1 texel/m: (42, 27).
        assert_eq!(raster.get(42, 27), COLOR_YOU_ABOVE);
        assert_eq!(raster.get(32, 32), [0, 0, 0, 0]);
    }

    #[test]
    fn parcel_raster_draws_lines_fills_and_borders() {
        let mut raster = LayerRaster::new(128);
        // A 64-cell region whose south-west corner sits 128 m south-west of the
        // layer centre: at 0.25 texels/m the region spans texels 32..=96.
        let cell = |row: usize, col: usize| -> Option<ParcelCell> {
            if row >= 64 || col >= 64 {
                return None;
            }
            let fill = if row == 4 && col == 4 {
                Some(ParcelFill::ForSale)
            } else if row == 4 && col == 8 {
                Some(ParcelFill::Auction)
            } else {
                None
            };
            Some(ParcelCell {
                fill,
                west_line: col == 4,
                south_line: row == 4,
            })
        };
        render_parcel_region(
            &mut raster,
            0.25,
            -128.0,
            -128.0,
            256.0,
            COLOR_PARCEL_LINE,
            true,
            false,
            64,
            &cell,
        );
        // Each 4 m cell is one texel; cell (4, 4) starts at texel (36, 36). Its
        // west line is the vertical line at x = 36.
        assert_eq!(raster.get(36, 36), COLOR_PARCEL_LINE);
        // The for-sale fill covers cell (4, 4)'s span (the shared corner texel
        // is overwritten by the line writes, so probe past it).
        assert_eq!(raster.get(37, 37), COLOR_FOR_SALE);
        // Cell (4, 8) is the auction fill.
        assert_eq!(raster.get(41, 37), COLOR_AUCTION);
        // The region's north border row (y = 32 + 64) and east border column.
        assert_eq!(raster.get(50, 96), COLOR_PARCEL_LINE);
        assert_eq!(raster.get(96, 50), COLOR_PARCEL_LINE);
    }

    #[test]
    fn parcel_fills_respect_the_for_sale_toggle() {
        let mut raster = LayerRaster::new(64);
        let cell = |row: usize, col: usize| -> Option<ParcelCell> {
            if row >= 64 || col >= 64 {
                return None;
            }
            Some(ParcelCell {
                fill: Some(ParcelFill::ForSale),
                ..ParcelCell::default()
            })
        };
        render_parcel_region(
            &mut raster,
            0.25,
            -128.0,
            -128.0,
            256.0,
            COLOR_PARCEL_LINE,
            false,
            false,
            64,
            &cell,
        );
        // No fill texels anywhere strictly inside the region.
        assert_eq!(raster.get(16, 16), [0, 0, 0, 0]);
    }

    #[test]
    fn overlay_less_region_draws_all_four_borders() {
        let mut raster = LayerRaster::new(128);
        let cell = |_row: usize, _col: usize| -> Option<ParcelCell> { None };
        render_parcel_region(
            &mut raster,
            0.25,
            -128.0,
            -128.0,
            256.0,
            COLOR_PARCEL_LINE,
            true,
            true,
            0,
            &cell,
        );
        // The region spans texels 32..=96: north and east borders as always…
        assert_eq!(raster.get(50, 96), COLOR_PARCEL_LINE);
        assert_eq!(raster.get(96, 50), COLOR_PARCEL_LINE);
        // …and, with no overlay to supply the edge property lines, the south
        // row and west column too.
        assert_eq!(raster.get(50, 32), COLOR_PARCEL_LINE);
        assert_eq!(raster.get(32, 50), COLOR_PARCEL_LINE);
    }

    #[test]
    fn compass_east_sits_on_the_right_edge_when_north_up() {
        let offset = compass_label_offset(0.0, 80.0, 80.0);
        assert!((offset.x - 80.0).abs() < 0.001);
        assert!(offset.y.abs() < 0.001);
    }

    #[test]
    fn compass_north_sits_on_the_top_edge_when_north_up() {
        let offset = compass_label_offset(core::f32::consts::FRAC_PI_2, 80.0, 80.0);
        assert!(offset.x.abs() < 0.001);
        // Screen y is down, so "top" is negative.
        assert!((offset.y + 80.0).abs() < 0.001);
    }

    #[test]
    fn compass_diagonal_projects_onto_the_corner() {
        let offset = compass_label_offset(core::f32::consts::FRAC_PI_4, 80.0, 60.0);
        // On a wider-than-tall rect a 45° direction leaves through the top edge.
        assert!((offset.y + 60.0).abs() < 0.001);
        assert!((offset.x - 60.0).abs() < 0.001);
    }

    #[test]
    fn minor_directions_hide_on_small_maps() {
        assert!(minor_directions_visible(12.0, Vec2::new(300.0, 300.0)));
        assert!(!minor_directions_visible(12.0, Vec2::new(100.0, 100.0)));
    }

    #[test]
    fn height_glyph_uses_the_seven_metre_band() {
        assert_eq!(height_glyph(0.0, false, 20.0), HeightGlyph::Level);
        assert_eq!(height_glyph(8.0, false, 20.0), HeightGlyph::Above);
        assert_eq!(height_glyph(-8.0, false, 20.0), HeightGlyph::Below);
        // Unknown altitude with a low camera reads as far above.
        assert_eq!(height_glyph(0.0, true, 20.0), HeightGlyph::Above);
        // Unknown altitude with the camera itself at the ceiling is unknown.
        assert_eq!(height_glyph(0.0, true, COARSE_MAX_Z), HeightGlyph::Unknown);
    }

    #[test]
    fn coarse_sentinels_read_as_unknown() {
        assert!(coarse_altitude_unknown(0.0));
        assert!(coarse_altitude_unknown(1020.0));
        assert!(!coarse_altitude_unknown(35.0));
    }

    #[test]
    fn dot_radius_floors_at_the_reference_minimum() {
        assert!((dot_radius(0.5) - 3.5).abs() < f32::EPSILON);
        assert!((dot_radius(16.0) - 12.0).abs() < f32::EPSILON);
    }

    #[test]
    fn avatar_glyphs_paint_their_shapes() {
        let mut data = vec![0u8; 32 * 32 * 4];
        let mut surface = Surface {
            width: 32,
            height: 32,
            data: &mut data,
        };
        draw_avatar_glyph(
            &mut surface,
            16.0,
            16.0,
            4.0,
            HeightGlyph::Level,
            [255, 0, 0, 255],
        );
        // The centre pixel is painted for a level dot.
        let offset = (16 * 32 + 16) * 4;
        assert_eq!(data.get(offset..offset + 4), Some(&[255u8, 0, 0, 255][..]));
    }

    #[test]
    fn double_click_action_decodes_the_setting() {
        assert_eq!(
            DoubleClickAction::from_setting(0),
            DoubleClickAction::Nothing
        );
        assert_eq!(
            DoubleClickAction::from_setting(1),
            DoubleClickAction::WorldMap
        );
        assert_eq!(
            DoubleClickAction::from_setting(2),
            DoubleClickAction::Teleport
        );
        assert_eq!(
            DoubleClickAction::from_setting(9),
            DoubleClickAction::Nothing
        );
    }
}
