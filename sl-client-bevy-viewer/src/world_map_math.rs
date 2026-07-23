//! Pure math for the world-map floater: the world↔surface transform, the two
//! zoom regimes and their map-tile levels, tile enumeration and sampling, and
//! the marker glyph drawing — no Bevy resources, no I/O, fully unit-tested.
//!
//! The reference viewer (Firestorm `llworldmapview.cpp` / `llworldmipmap.cpp`,
//! read-only reference) renders the map at a *scale* measured in pixels per
//! 256 m region and derives the tile mipmap level from it
//! (`LLWorldMipmap::scaleToLevel`): level 1 tiles cover one region each, and
//! each level up doubles the region span per 256 px tile, up to level 8
//! (128 regions per tile edge). Region names and per-region overlays only draw
//! in the *detail* regime (level ≤ 3, `DRAW_SIMINFO_THRESHOLD`); the grid-wide
//! regime shows the tile imagery alone. The same thresholds are used here.
//!
//! Shared raster plumbing ([`Rgba`], [`Surface`], the disc / ring glyphs)
//! comes from [`crate::minimap_math`]; this module adds what the world map
//! needs on top: an absolute (global-metre-centred, north-up) view transform
//! instead of the minimap's camera-relative rotating one, and the tile
//! geometry.

use bevy::math::Vec2;

use crate::minimap_math::{REGION_WIDTH_METRES, Rgba, Surface, round_i32};

// ---------------------------------------------------------------------------
// Scale (zoom) regime.
// ---------------------------------------------------------------------------

/// The smallest map scale (pixels per region): the whole-grid overview, one
/// pixel per region (the reference zoom slider's minimum, `exp2(-8)·256`).
pub(crate) const WORLD_MAP_SCALE_MIN: f32 = 1.0;

/// The largest map scale (pixels per region): the reference zoom slider's
/// maximum (`exp2(0)·256`).
pub(crate) const WORLD_MAP_SCALE_MAX: f32 = 256.0;

/// The default map scale (the reference `MAP_DEFAULT_SCALE`).
pub(crate) const WORLD_MAP_SCALE_DEFAULT: f32 = 128.0;

/// The "Close" zoom preset (one region fills 256 px).
pub(crate) const WORLD_MAP_SCALE_CLOSE: f32 = 256.0;

/// The "Medium" zoom preset.
pub(crate) const WORLD_MAP_SCALE_MEDIUM: f32 = 128.0;

/// The "Far" zoom preset.
pub(crate) const WORLD_MAP_SCALE_FAR: f32 = 32.0;

/// The "Grid" zoom preset: a wide overview (32 regions per 128 px).
pub(crate) const WORLD_MAP_SCALE_GRID: f32 = 4.0;

/// One scroll-wheel notch changes the zoom exponent by this much (the
/// reference world map moves its zoom slider a quarter step per notch, a
/// `2^0.25` scale factor).
pub(crate) const WHEEL_ZOOM_STEP: f32 = 0.25;

/// Clamps a world-map scale to its valid range.
pub(crate) const fn clamp_world_scale(scale: f32) -> f32 {
    scale.clamp(WORLD_MAP_SCALE_MIN, WORLD_MAP_SCALE_MAX)
}

/// The scale after `clicks` scroll-wheel notches (positive zooms in).
pub(crate) fn wheel_world_scale(scale: f32, clicks: f32) -> f32 {
    clamp_world_scale(scale * (clicks * WHEEL_ZOOM_STEP).exp2())
}

// ---------------------------------------------------------------------------
// Tile levels.
// ---------------------------------------------------------------------------

/// The number of map-tile mipmap levels the map servers render
/// (`LLWorldMipmap::MAP_LEVELS`).
pub(crate) const MAX_TILE_LEVEL: u8 = 8;

/// The highest (coarsest) tile level at which per-region information (names,
/// map blocks, item markers) is still requested and drawn — the reference's
/// `DRAW_SIMINFO_THRESHOLD`.
pub(crate) const SIM_INFO_MAX_LEVEL: u8 = 3;

/// The smallest scale (pixels per region) at which region-name labels draw
/// (the reference draws sim names from about this scale up).
pub(crate) const REGION_NAME_MIN_SCALE: f32 = 96.0;

/// The tile mipmap level for a map scale (`LLWorldMipmap::scaleToLevel`):
/// level 1 below one-to-one, one level per halving of the scale, clamped to
/// `1..=`[`MAX_TILE_LEVEL`].
pub(crate) fn tile_level(scale: f32) -> u8 {
    if scale <= f32::EPSILON {
        return MAX_TILE_LEVEL;
    }
    let level = (REGION_WIDTH_METRES / scale).log2().floor() + 1.0;
    let clamped = level.clamp(1.0, f32::from(MAX_TILE_LEVEL));
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [1, 8] just above"
    )]
    let out = clamped as u8;
    out
}

/// Whether a scale is in the region-detail regime (per-region info drawn).
pub(crate) fn detail_regime(scale: f32) -> bool {
    tile_level(scale) <= SIM_INFO_MAX_LEVEL
}

/// The regions per tile edge at a level (`2^(level-1)`).
pub(crate) fn tile_span_regions(level: u8) -> u32 {
    1_u32 << u32::from(level.clamp(1, MAX_TILE_LEVEL).saturating_sub(1))
}

/// The lower-left grid coordinate of the tile containing a grid coordinate at
/// a level (coordinates snap down to the tile span).
pub(crate) fn tile_corner(level: u8, grid_x: u32, grid_y: u32) -> (u32, u32) {
    let span = tile_span_regions(level).max(1);
    (
        grid_x.checked_div(span).unwrap_or(0).saturating_mul(span),
        grid_y.checked_div(span).unwrap_or(0).saturating_mul(span),
    )
}

/// The distinct tile corners covering an inclusive grid-coordinate rectangle
/// at a level, capped at `cap` tiles (a runaway guard for huge view rects).
pub(crate) fn tiles_in_rect(
    level: u8,
    min_x: u32,
    max_x: u32,
    min_y: u32,
    max_y: u32,
    cap: usize,
) -> Vec<(u32, u32)> {
    let span = tile_span_regions(level);
    let (start_x, start_y) = tile_corner(level, min_x, min_y);
    let mut tiles = Vec::new();
    let mut y = start_y;
    while y <= max_y {
        let mut x = start_x;
        while x <= max_x {
            if tiles.len() >= cap {
                return tiles;
            }
            tiles.push((x, y));
            let Some(next) = x.checked_add(span) else {
                break;
            };
            x = next;
        }
        let Some(next) = y.checked_add(span) else {
            break;
        };
        y = next;
    }
    tiles
}

/// Splits an inclusive grid-coordinate rectangle into request chunks of at
/// most `chunk`×`chunk` regions (the map-block request granularity), capped at
/// `cap` chunks.
pub(crate) fn request_chunks(
    min_x: u32,
    max_x: u32,
    min_y: u32,
    max_y: u32,
    chunk: u32,
    cap: usize,
) -> Vec<(u32, u32, u32, u32)> {
    let chunk = chunk.max(1);
    let mut chunks = Vec::new();
    let mut y = min_y.checked_div(chunk).unwrap_or(0).saturating_mul(chunk);
    while y <= max_y {
        let mut x = min_x.checked_div(chunk).unwrap_or(0).saturating_mul(chunk);
        while x <= max_x {
            if chunks.len() >= cap {
                return chunks;
            }
            chunks.push((
                x.max(min_x),
                x.saturating_add(chunk.saturating_sub(1)).min(max_x),
                y.max(min_y),
                y.saturating_add(chunk.saturating_sub(1)).min(max_y),
            ));
            let Some(next) = x.checked_add(chunk) else {
                break;
            };
            x = next;
        }
        let Some(next) = y.checked_add(chunk) else {
            break;
        };
        y = next;
    }
    chunks
}

// ---------------------------------------------------------------------------
// The view transform.
// ---------------------------------------------------------------------------

/// The world-map view: an absolute global-metre centre, a scale in pixels per
/// region, and the surface size in pixels. Always north-up (the reference
/// world map never rotates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WorldMapView {
    /// The global metres east at the surface centre.
    pub(crate) center_east: f64,
    /// The global metres north at the surface centre.
    pub(crate) center_north: f64,
    /// The scale, in pixels per 256 m region.
    pub(crate) scale: f32,
    /// The surface size, in pixels.
    pub(crate) size: Vec2,
}

impl WorldMapView {
    /// Pixels per metre at the current scale.
    pub(crate) fn pixels_per_metre(&self) -> f32 {
        self.scale / REGION_WIDTH_METRES
    }

    /// A global position as a surface pixel (top-left origin, y down).
    pub(crate) fn view_from_global(&self, east: f64, north: f64) -> Vec2 {
        let ppm = f64::from(self.pixels_per_metre());
        let x = f64::from(self.size.x) / 2.0 + (east - self.center_east) * ppm;
        let y = f64::from(self.size.y) / 2.0 - (north - self.center_north) * ppm;
        Vec2::new(narrow_f64(x), narrow_f64(y))
    }

    /// The global position (metres east, north) under a surface pixel.
    pub(crate) fn global_from_view(&self, view: Vec2) -> (f64, f64) {
        let ppm = f64::from(self.pixels_per_metre());
        let east = self.center_east + (f64::from(view.x) - f64::from(self.size.x) / 2.0) / ppm;
        let north = self.center_north - (f64::from(view.y) - f64::from(self.size.y) / 2.0) / ppm;
        (east, north)
    }

    /// The inclusive grid-coordinate rectangle the surface shows (clamped to
    /// the representable grid).
    pub(crate) fn visible_grid_rect(&self) -> (u32, u32, u32, u32) {
        let (min_east, max_north) = self.global_from_view(Vec2::new(0.0, 0.0));
        let (max_east, min_north) = self.global_from_view(self.size);
        (
            grid_index(min_east),
            grid_index(max_east),
            grid_index(min_north),
            grid_index(max_north),
        )
    }
}

/// The centre that keeps the global point under `cursor` fixed across a scale
/// change (zoom-to-cursor).
pub(crate) fn zoom_center(view: &WorldMapView, cursor: Vec2, new_scale: f32) -> (f64, f64) {
    let (east, north) = view.global_from_view(cursor);
    let ppm = f64::from(new_scale / REGION_WIDTH_METRES);
    let center_east = east - (f64::from(cursor.x) - f64::from(view.size.x) / 2.0) / ppm;
    let center_north = north + (f64::from(cursor.y) - f64::from(view.size.y) / 2.0) / ppm;
    (center_east, center_north)
}

/// The grid index containing a global metre coordinate, clamped to the
/// representable grid (negative and non-finite clamp to zero).
pub(crate) fn grid_index(meters: f64) -> u32 {
    if !meters.is_finite() || meters <= 0.0 {
        return 0;
    }
    let index = (meters / f64::from(REGION_WIDTH_METRES)).floor();
    if index >= f64::from(u32::MAX) {
        return u32::MAX;
    }
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "range-checked to [0, u32::MAX) just above"
    )]
    let out = index as u32;
    out
}

/// Narrows an `f64` surface-pixel coordinate to `f32` (surface coordinates are
/// bounded by the widget size, far inside `f32` precision).
pub(crate) const fn narrow_f64(value: f64) -> f32 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "surface pixel coordinates are small; sub-pixel precision loss is irrelevant"
    )]
    let out = value as f32;
    out
}

// ---------------------------------------------------------------------------
// Tile rasters.
// ---------------------------------------------------------------------------

/// A decoded map tile: RGBA8 texels in top-down row order (as image decoders
/// produce), covering `tile_span_regions(level)` regions square from the
/// tile's lower-left corner.
#[derive(Debug, Clone, Default)]
pub(crate) struct TileRaster {
    /// The raster width in texels.
    pub(crate) width: u32,
    /// The raster height in texels.
    pub(crate) height: u32,
    /// RGBA8 texels, top-down rows.
    pub(crate) data: Vec<u8>,
}

impl TileRaster {
    /// The texel at (x, y) (top-down row order), transparent black outside.
    pub(crate) fn get(&self, x: i32, y: i32) -> Rgba {
        if x < 0 || y < 0 {
            return [0, 0, 0, 0];
        }
        let (x, y) = (u32_from_i32(x), u32_from_i32(y));
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 0];
        }
        let offset = usize::try_from(y)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(self.width).unwrap_or(0))
            .saturating_add(usize::try_from(x).unwrap_or(0))
            .saturating_mul(4);
        self.data
            .get(offset..offset.saturating_add(4))
            .map_or([0, 0, 0, 0], |slice| match *slice {
                [r, g, b, a] => [r, g, b, a],
                _ => [0, 0, 0, 0],
            })
    }

    /// Samples the raster at a global position for a tile anchored at grid
    /// corner (`corner_x`, `corner_y`) with the given level's span. Positions
    /// outside the tile clamp to its edge texels.
    pub(crate) fn sample(
        &self,
        level: u8,
        corner_x: u32,
        corner_y: u32,
        east: f64,
        north: f64,
    ) -> Rgba {
        let span_metres = f64::from(tile_span_regions(level)) * f64::from(REGION_WIDTH_METRES);
        let east_frac = ((east - f64::from(corner_x) * f64::from(REGION_WIDTH_METRES))
            / span_metres)
            .clamp(0.0, 1.0);
        let north_frac = ((north - f64::from(corner_y) * f64::from(REGION_WIDTH_METRES))
            / span_metres)
            .clamp(0.0, 1.0);
        // Row 0 is the tile's north edge.
        let x = round_i32(narrow_f64(east_frac * f64::from(self.width) - 0.5));
        let y = round_i32(narrow_f64(
            (1.0 - north_frac) * f64::from(self.height) - 0.5,
        ));
        self.get(
            x.clamp(0, i32_from_u32(self.width.saturating_sub(1))),
            y.clamp(0, i32_from_u32(self.height.saturating_sub(1))),
        )
    }
}

/// A non-negative `i32` as `u32` (callers range-check first).
const fn u32_from_i32(value: i32) -> u32 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_sign_loss,
        reason = "callers check for a non-negative value first"
    )]
    let out = value as u32;
    out
}

/// A `u32` as `i32`, saturating at `i32::MAX`.
fn i32_from_u32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

// ---------------------------------------------------------------------------
// Colours and glyphs.
// ---------------------------------------------------------------------------

/// The map background where nothing is known (deep off-grid water).
pub(crate) const COLOR_MAP_VOID: Rgba = [16, 27, 44, 255];

/// The region-border grid line drawn in the detail regime.
pub(crate) const COLOR_MAP_GRID_LINE: Rgba = [255, 255, 255, 26];

/// Avatar ("people") map items.
pub(crate) const COLOR_MAP_AGENT: Rgba = [0, 228, 0, 255];

/// Telehub / infohub map items.
pub(crate) const COLOR_MAP_TELEHUB: Rgba = [128, 96, 255, 255];

/// Land-for-sale map items.
pub(crate) const COLOR_MAP_LAND_SALE: Rgba = [255, 231, 100, 255];

/// Adult land-for-sale map items.
pub(crate) const COLOR_MAP_LAND_SALE_ADULT: Rgba = [255, 128, 64, 255];

/// PG event map items.
pub(crate) const COLOR_MAP_EVENT: Rgba = [255, 128, 200, 255];

/// Mature event map items.
pub(crate) const COLOR_MAP_EVENT_MATURE: Rgba = [255, 64, 128, 255];

/// Adult event map items.
pub(crate) const COLOR_MAP_EVENT_ADULT: Rgba = [200, 0, 64, 255];

/// The own-avatar marker.
pub(crate) const COLOR_MAP_SELF: Rgba = [255, 255, 0, 255];

/// Draws a filled axis-aligned square glyph (the land-for-sale marker shape).
pub(crate) fn draw_square(surface: &mut Surface<'_>, cx: f32, cy: f32, half: f32, color: Rgba) {
    let min_x = round_i32(cx - half);
    let max_x = round_i32(cx + half);
    let min_y = round_i32(cy - half);
    let max_y = round_i32(cy + half);
    let mut y = min_y;
    while y <= max_y {
        let mut x = min_x;
        while x <= max_x {
            surface.blend(x, y, color);
            x = x.saturating_add(1);
        }
        y = y.saturating_add(1);
    }
}

/// Draws a 1 px vertical line (a region border in the detail regime).
pub(crate) fn draw_vline(surface: &mut Surface<'_>, x: f32, color: Rgba) {
    let x = round_i32(x);
    let mut y = 0_i32;
    let height = i32_from_u32(surface.height);
    while y < height {
        surface.blend(x, y, color);
        y = y.saturating_add(1);
    }
}

/// Draws a 1 px horizontal line (a region border in the detail regime).
pub(crate) fn draw_hline(surface: &mut Surface<'_>, y: f32, color: Rgba) {
    let y = round_i32(y);
    let mut x = 0_i32;
    let width = i32_from_u32(surface.width);
    while x < width {
        surface.blend(x, y, color);
        x = x.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_TILE_LEVEL, TileRaster, WHEEL_ZOOM_STEP, WORLD_MAP_SCALE_MAX, WORLD_MAP_SCALE_MIN,
        WorldMapView, clamp_world_scale, detail_regime, grid_index, request_chunks, tile_corner,
        tile_level, tile_span_regions, tiles_in_rect, wheel_world_scale, zoom_center,
    };
    use bevy::math::Vec2;
    use pretty_assertions::assert_eq;

    /// A 512×512 view centred on the middle of region (1000, 1000).
    fn view() -> WorldMapView {
        WorldMapView {
            center_east: 1000.0 * 256.0 + 128.0,
            center_north: 1000.0 * 256.0 + 128.0,
            scale: 128.0,
            size: Vec2::new(512.0, 512.0),
        }
    }

    #[test]
    fn tile_levels_follow_the_reference_scale_mapping() {
        // Full detail at one-to-one and above.
        assert_eq!(tile_level(256.0), 1);
        assert_eq!(tile_level(129.0), 1);
        // One level per halving.
        assert_eq!(tile_level(128.0), 2);
        assert_eq!(tile_level(64.0), 3);
        assert_eq!(tile_level(32.0), 4);
        assert_eq!(tile_level(16.0), 5);
        assert_eq!(tile_level(8.0), 6);
        assert_eq!(tile_level(4.0), 7);
        assert_eq!(tile_level(2.0), 8);
        // Clamped at the coarsest level.
        assert_eq!(tile_level(1.0), MAX_TILE_LEVEL);
        assert_eq!(tile_level(0.0), MAX_TILE_LEVEL);
    }

    #[test]
    fn detail_regime_matches_the_siminfo_threshold() {
        assert!(detail_regime(256.0));
        assert!(detail_regime(128.0));
        assert!(detail_regime(64.0));
        assert!(!detail_regime(32.0));
        assert!(!detail_regime(1.0));
    }

    #[test]
    fn tile_spans_double_per_level() {
        assert_eq!(tile_span_regions(1), 1);
        assert_eq!(tile_span_regions(2), 2);
        assert_eq!(tile_span_regions(3), 4);
        assert_eq!(tile_span_regions(8), 128);
    }

    #[test]
    fn tile_corners_snap_down() {
        assert_eq!(tile_corner(1, 1000, 1001), (1000, 1001));
        assert_eq!(tile_corner(2, 1001, 1001), (1000, 1000));
        assert_eq!(tile_corner(3, 1003, 1005), (1000, 1004));
        assert_eq!(tile_corner(8, 1000, 1000), (896, 896));
    }

    #[test]
    fn tiles_cover_the_rect() {
        let tiles = tiles_in_rect(2, 999, 1002, 1000, 1001, 64);
        assert_eq!(tiles, vec![(998, 1000), (1000, 1000), (1002, 1000)]);
        // The cap bounds a runaway rect.
        assert_eq!(tiles_in_rect(1, 0, 10_000, 0, 10_000, 5).len(), 5);
    }

    #[test]
    fn request_chunks_align_and_clip() {
        let chunks = request_chunks(999, 1002, 1000, 1001, 8, 64);
        // The visible rect straddles the 8-region chunk boundary at 1000.
        assert_eq!(
            chunks,
            vec![(999, 999, 1000, 1001), (1000, 1002, 1000, 1001)]
        );
        assert_eq!(request_chunks(0, 10_000, 0, 10_000, 8, 3).len(), 3);
    }

    #[test]
    fn view_transform_round_trips() {
        let view = view();
        let centre = view.view_from_global(view.center_east, view.center_north);
        assert!((centre.x - 256.0).abs() < 0.01);
        assert!((centre.y - 256.0).abs() < 0.01);
        // One region east = +128 px at scale 128; north is up (y down).
        let east = view.view_from_global(view.center_east + 256.0, view.center_north + 256.0);
        assert!((east.x - 384.0).abs() < 0.01);
        assert!((east.y - 128.0).abs() < 0.01);
        let (e, n) = view.global_from_view(Vec2::new(384.0, 128.0));
        assert!((e - (view.center_east + 256.0)).abs() < 0.5);
        assert!((n - (view.center_north + 256.0)).abs() < 0.5);
    }

    #[test]
    fn visible_rect_covers_the_view() {
        let (min_x, max_x, min_y, max_y) = view().visible_grid_rect();
        // 512 px at 128 px/region = 4 regions across, centred on 1000.
        assert_eq!((min_x, max_x), (998, 1002));
        assert_eq!((min_y, max_y), (998, 1002));
    }

    #[test]
    fn zoom_keeps_the_cursor_point_fixed() {
        let mut view = view();
        let cursor = Vec2::new(100.0, 400.0);
        let (before_e, before_n) = view.global_from_view(cursor);
        let new_scale = 256.0;
        let (center_east, center_north) = zoom_center(&view, cursor, new_scale);
        view.center_east = center_east;
        view.center_north = center_north;
        view.scale = new_scale;
        let (after_e, after_n) = view.global_from_view(cursor);
        assert!((before_e - after_e).abs() < 0.01);
        assert!((before_n - after_n).abs() < 0.01);
    }

    #[test]
    fn grid_index_clamps() {
        assert_eq!(grid_index(-5.0), 0);
        assert_eq!(grid_index(0.0), 0);
        assert_eq!(grid_index(255.9), 0);
        assert_eq!(grid_index(256_000.0), 1000);
        assert_eq!(grid_index(f64::NAN), 0);
    }

    #[test]
    fn wheel_zoom_steps_and_clamps() {
        let one_notch = wheel_world_scale(128.0, 1.0);
        assert!((one_notch - 128.0 * WHEEL_ZOOM_STEP.exp2()).abs() < 0.01);
        let clamped_up = wheel_world_scale(WORLD_MAP_SCALE_MAX, 10.0);
        assert!((clamped_up - WORLD_MAP_SCALE_MAX).abs() < f32::EPSILON);
        let clamped_down = wheel_world_scale(WORLD_MAP_SCALE_MIN, -10.0);
        assert!((clamped_down - WORLD_MAP_SCALE_MIN).abs() < f32::EPSILON);
        assert!((clamp_world_scale(9999.0) - WORLD_MAP_SCALE_MAX).abs() < f32::EPSILON);
    }

    #[test]
    fn tile_sampling_flips_rows() {
        // A 2×2 tile at level 1: distinct texels per quadrant.
        let raster = TileRaster {
            width: 2,
            height: 2,
            data: vec![
                1, 0, 0, 255, /* top-left (NW) */ 2, 0, 0, 255, /* top-right (NE) */
                3, 0, 0, 255, /* bottom-left (SW) */ 4, 0, 0,
                255, /* bottom-right (SE) */
            ],
        };
        let base_e = 1000.0 * 256.0;
        let base_n = 1000.0 * 256.0;
        // South-west quarter of the region → bottom-left texel.
        assert_eq!(
            raster.sample(1, 1000, 1000, base_e + 64.0, base_n + 64.0)[0],
            3
        );
        // North-east quarter → top-right texel.
        assert_eq!(
            raster.sample(1, 1000, 1000, base_e + 192.0, base_n + 192.0)[0],
            2
        );
        // Outside clamps to the edge.
        assert_eq!(
            raster.sample(1, 1000, 1000, base_e - 50.0, base_n + 192.0)[0],
            1
        );
    }
}
