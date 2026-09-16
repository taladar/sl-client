//! The **sculpt surface**: reading a decoded RGB sculpt map as a grid of vertex
//! positions and laying it over the prim's own path and profile.
//!
//! A sculpt is not a shape of its own. The reference viewer builds a sculpted
//! prim's volume from the prim's **path and profile** exactly as for a plain
//! prim — only asking a circular path and a circle profile for as many steps as
//! the map is worth — and then, instead of sweeping the profile along the path,
//! reads every vertex position out of the map. The shape parameters therefore
//! decide how deep and how wide the vertex grid is, how many faces the prim has
//! and which of them are caps, and every texture coordinate; the map decides
//! only where the vertices are.
//!
//! So [`tessellate`]:
//!
//! 1. sizes the grid with [`mesh_resolution`] (Firestorm's
//!    `sculpt_calc_mesh_resolution`) and generates the sculpt's path and profile
//!    for it ([`Path::generate_sculpted`], [`Profile::generate_sculpted`]);
//! 2. reads one map texel per grid vertex (`sculptGenerateMapVertices`) — the
//!    nearest texel below the vertex's fraction of the map, the sphere type's
//!    first and last rows pinched to the middle column, the wrapping types'
//!    last column wrapping to the first — mapping `(r, g, b) / 255 - 0.5` to a
//!    position in Second Life's right-handed **Z-up** space;
//! 3. rejects a surface with too little or too much area in favour of a sphere
//!    placeholder, and a map with no usable data in favour of an empty one
//!    (`LLVolume::sculpt`);
//! 4. hands the grid to [`tessellate_sculpted`], which builds the faces the
//!    profile names and stitches the side normals the way the sculpt type asks.
//!
//! A seam is two vertices, not one: the first and last column carry the same
//! position but texture coordinates `0` and `1`, so the texture runs once around
//! instead of squeezing a reversed copy into the last column. A pole is a whole
//! row of coincident vertices whose normals are averaged.
//!
//! On the usual sculpt shape — circle profile, circle path, what the build tool
//! and `PRIM_TYPE_SCULPT` both set — that is one closed side face. On any other
//! shape it is whatever that shape's faces are: a box's line path is two rows
//! deep whatever the map, so a sculpt left on a box collapses to its poles,
//! fails the area test, and shows the placeholder's half-disc on a cap. That is
//! what the reference draws, and what content made against it expects.
//!
//! **The grid's rows run bottom-up through the visible map.** The reference
//! viewer's JPEG2000 decoder copies rows *bottom-up* into `LLImageRaw` (row 0 =
//! the visible bottom), and `sculptGenerateMapVertices` reads row `y` from there;
//! a [`DecodedImage`] is top-down, so the row is flipped when the texel is read.
//! Reading top-down instead builds every sculpt as its own mirror image —
//! winding inverted relative to the back-face cull, so real-convention sculpt
//! content renders inside out (the aditi pillows bug).
//!
//! It is a faithful, idiomatic re-implementation of Firestorm
//! `indra/llmath/llvolume.cpp` — `LLVolume::sculpt`, `sculpt_calc_mesh_resolution`,
//! `sculptGenerateMapVertices`, `sculptGetSurfaceArea` and the two placeholders —
//! reworked to the workspace's restriction lints (no indexing, no `as` casts
//! outside the bounded numeric helpers, no panics).

use crate::stitch::{SculptParams, SculptStitch};
use sl_prim::{
    PRIM_LOD_COUNT, Path, PrimLod, PrimMesh, PrimShape, Profile, SculptSeams, SculptStitching,
    tessellate_sculpted,
};
use sl_texture::DecodedImage;

/// The number of quad cells per side of the finest sculpt working grid
/// (Firestorm's `SCULPT_REZ_4`), the ceiling [`mesh_resolution`] works down from.
///
/// A circle path or profile asked for that many steps has `MAX_SUBDIVISIONS + 1`
/// points: the seam is two of them.
pub const MAX_SUBDIVISIONS: usize = 32;

/// The per-side cell counts of the four sculpt levels of detail, coarsest first —
/// Firestorm's `SCULPT_REZ_1..4`, indexed by [`PrimLod`].
///
/// `SCULPT_REZ_1` is `6` rather than the `4` the original code used; the
/// reference's own comment explains why: "6 looks round whereas 4 looks square".
const SCULPT_REZ: [usize; PRIM_LOD_COUNT] = [6, 8, 16, MAX_SUBDIVISIONS];

/// The smallest per-axis cell count a sculpt grid is allowed, so a very wide or
/// very tall map cannot collapse one axis to nothing (Firestorm's "no degenerate
/// sizes, please").
const MIN_SUBDIVISIONS: usize = 4;

/// The number of map pixels each grid vertex is worth (Firestorm's
/// `width * height / 4`): a sculpt is never tessellated finer than a quarter of
/// its map's pixel count, because the extra vertices would carry no new
/// displacement.
const PIXELS_PER_VERTEX: usize = 4;

/// The per-side cell count of the sculpt grid at `lod` — Firestorm's
/// `sculpt_sides`, which selects among `SCULPT_REZ_1..4` by the volume's detail
/// multiplier rather than always taking the finest.
fn sculpt_sides(lod: PrimLod) -> usize {
    let detail = lod.detail();
    let tier = if detail <= 1.0 {
        0
    } else if detail <= 2.0 {
        1
    } else if detail <= 3.0 {
        2
    } else {
        3
    };
    SCULPT_REZ.get(tier).copied().unwrap_or(MAX_SUBDIVISIONS)
}

/// The working grid a `width`×`height` sculpt map is resampled onto at `lod`, as
/// `(rows, columns)` quad cells — a faithful port of Firestorm's
/// `sculpt_calc_mesh_resolution`.
///
/// Rows run along the map's height (the reference's path / `sizeS` axis) and
/// columns along its width (its profile / `sizeT` axis). The reference states the
/// three properties it balances: the grid's aspect ratio tracks the map's as
/// closely as it can while still spending every vertex the budget allows; the
/// budget is capped by the level of detail; and it is capped again by the map,
/// since a vertex per fewer than four pixels only resamples
/// displacement that is already there.
///
/// Both counts are at least four. A zero-sized map (which
/// [`tessellate`] renders as a placeholder anyway) is treated as square and sized
/// by the level of detail alone.
#[must_use]
pub fn mesh_resolution(width: u32, height: u32, lod: PrimLod) -> (usize, usize) {
    let sides = sculpt_sides(lod);
    let max_vertices_lod = sides.saturating_mul(sides);
    let max_vertices_map = usize_from_u32(width)
        .saturating_mul(usize_from_u32(height))
        .checked_div(PIXELS_PER_VERTEX)
        .unwrap_or(0);
    let vertices = if max_vertices_map > 0 {
        max_vertices_lod.min(max_vertices_map)
    } else {
        max_vertices_lod
    };

    let ratio = if width == 0 || height == 0 {
        1.0
    } else {
        f32_from_usize(usize_from_u32(width)) / f32_from_usize(usize_from_u32(height))
    };

    // Split the vertex budget between the axes in the map's aspect ratio, then
    // let integer division give the other axis every vertex the first leaves.
    let mut rows = usize_from_f32_floor((f32_from_usize(vertices) / ratio).sqrt());
    rows = rows.max(MIN_SUBDIVISIONS);
    let columns = vertices
        .checked_div(rows)
        .unwrap_or(0)
        .max(MIN_SUBDIVISIONS);
    rows = vertices
        .checked_div(columns)
        .unwrap_or(0)
        .max(MIN_SUBDIVISIONS);
    (rows, columns)
}

/// The number of bytes per pixel in a decoded [`DecodedImage`] (canonical RGBA8).
const RGBA_CHANNELS: usize = 4;

/// The fewest source components a sculpt map must have to carry a position per
/// texel (Firestorm's `sculpt_components < 3` rejection): a grey map is no map.
const MIN_COMPONENTS: u16 = 3;

/// The inverse of the 8-bit channel range, mapping `0..=255` to `0.0..=1.0`.
const INV_U8_MAX: f32 = 1.0 / 255.0;

/// The centre offset subtracted from a normalised channel to place the origin at
/// the middle of the sculpt cube (Firestorm's `sub(0.5)`).
const CHANNEL_CENTRE: f32 = 0.5;

/// The radius of the sphere placeholder a surface that fails the area test is
/// replaced by (Firestorm's `sculptGenerateSpherePlaceholder`).
const PLACEHOLDER_RADIUS: f32 = 0.3;

/// The least surface area a sculpt may have before it is judged degenerate
/// (Firestorm's `SCULPT_MIN_AREA`).
const SCULPT_MIN_AREA: f32 = 0.002;

/// The most surface area a sculpt may have before it is judged degenerate
/// (Firestorm's `SCULPT_MAX_AREA`).
const SCULPT_MAX_AREA: f32 = 384.0;

/// The detail at or below which the area test is skipped, so the lowest level
/// keeps legacy content that only works coarse (Firestorm's
/// `SCULPT_MIN_AREA_DETAIL`, "don't test lowest LOD to support legacy content").
const SCULPT_MIN_AREA_DETAIL: f32 = 1.0;

/// Tessellate a sculpted prim at `lod`: its decoded sculpt `map`, stitched
/// according to the wire `sculpt_type` byte, laid over the path and profile of
/// its `shape`.
///
/// The byte's low bits select the [`SculptStitch`] and its high bits the
/// invert / mirror flags (see [`SculptParams`]). See the module docs for what
/// the shape contributes; the result has one [`sl_prim::PrimFace`] per face the
/// shape's profile names, in Linden face order.
#[must_use]
pub fn tessellate(
    map: &DecodedImage,
    sculpt_type: u8,
    shape: &PrimShape,
    lod: PrimLod,
) -> PrimMesh {
    tessellate_with(map, SculptParams::from_sculpt_type(sculpt_type), shape, lod)
}

/// [`tessellate`] with the `sculpt_type` byte already parsed into
/// [`SculptParams`].
///
/// A map that carries no positions — zero-sized, fewer than three components,
/// or shorter than its geometry — gives the reference's empty placeholder: a
/// surface of coincident vertices that draws nothing. A surface whose area is
/// implausible gives its visible sphere placeholder instead.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `tessellate_with` reads clearly"
)]
pub fn tessellate_with(
    map: &DecodedImage,
    params: SculptParams,
    shape: &PrimShape,
    lod: PrimLod,
) -> PrimMesh {
    // The requested sizes come from the declared map size even when its data
    // turns out unusable, as in the reference.
    let (rows, columns) = mesh_resolution(map.width, map.height, lod);
    let path = Path::generate_sculpted(shape, lod, rows);
    let profile = Profile::generate_sculpted(shape, lod, path.is_open(), columns);
    let size_s = path.point_count();
    let size_t = profile.point_count();

    let surface = match SculptMap::new(map) {
        None => vec![[0.0; 3]; size_s.saturating_mul(size_t)],
        Some(sculpt) => {
            let surface = sculpt.surface(params, size_s, size_t);
            let area = surface_area(&surface, size_s, size_t);
            if lod.detail() > SCULPT_MIN_AREA_DETAIL
                && !(SCULPT_MIN_AREA..=SCULPT_MAX_AREA).contains(&area)
            {
                sphere_placeholder(size_s, size_t)
            } else {
                surface
            }
        }
    };
    tessellate_sculpted(shape, &path, &profile, surface, stitching(params))
}

/// The side-face normal stitching and texture reversal a sculpt type asks for
/// (the sculpt branch of Firestorm's `createSide`).
const fn stitching(params: SculptParams) -> SculptStitching {
    SculptStitching {
        seams: match params.stitch {
            SculptStitch::Plane => SculptSeams::Open,
            SculptStitch::Cylinder => SculptSeams::Cylinder,
            SculptStitch::Sphere => SculptSeams::Sphere,
            SculptStitch::Torus => SculptSeams::Torus,
        },
        reverse_u: params.reverse_u(),
    }
}

/// A borrowed view over a decoded sculpt map's RGBA8 pixels.
struct SculptMap<'pixels> {
    /// The map width in pixels.
    width: usize,
    /// The map height in pixels.
    height: usize,
    /// The tightly packed RGBA8 pixels, row-major and top-down
    /// (`(y * width + x) * 4`).
    pixels: &'pixels [u8],
}

impl<'pixels> SculptMap<'pixels> {
    /// View `image` as a sculpt map, or `None` when it carries no positions —
    /// zero width or height, fewer than three source components, or fewer
    /// pixel bytes than its geometry requires.
    fn new(image: &'pixels DecodedImage) -> Option<Self> {
        let width = usize::try_from(image.width).ok()?;
        let height = usize::try_from(image.height).ok()?;
        if width == 0 || height == 0 || image.components < MIN_COMPONENTS {
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
            pixels: &image.pixels,
        })
    }

    /// The `size_s × size_t` surface grid, row-major — path row outer, profile
    /// column inner (Firestorm `sculptGenerateMapVertices`).
    fn surface(&self, params: SculptParams, size_s: usize, size_t: usize) -> Vec<[f32; 3]> {
        let mut surface = Vec::with_capacity(size_s.saturating_mul(size_t));
        for s in 0..size_s {
            for t in 0..size_t {
                let column = if params.reverse_u() {
                    size_t.saturating_sub(t).saturating_sub(1)
                } else {
                    t
                };
                let (x, y) = self.texel_for(params.stitch, column, size_t, s, size_s);
                let position = self.texel(x, y);
                surface.push(if params.mirror {
                    [-position[0], position[1], position[2]]
                } else {
                    position
                });
            }
        }
        surface
    }

    /// The texel `(x, y)` — `y` counted from the visible bottom — that grid
    /// vertex `(column, s)` reads: the texel below its fraction of the map, with
    /// the stitch type's pinch and wrap applied to the map's far edges.
    fn texel_for(
        &self,
        stitch: SculptStitch,
        column: usize,
        size_t: usize,
        s: usize,
        size_s: usize,
    ) -> (usize, usize) {
        let mut x = grid_to_texel(column, size_t, self.width);
        let mut y = grid_to_texel(s, size_s, self.height);
        let middle = self.width.checked_div(2).unwrap_or(0);
        if y == 0 && stitch.has_poles() {
            x = middle;
        }
        if y == self.height {
            y = if stitch.wraps_v() {
                0
            } else {
                self.height.saturating_sub(1)
            };
            if stitch.has_poles() {
                x = middle;
            }
        }
        if x == self.width {
            x = if stitch.wraps_u() {
                0
            } else {
                self.width.saturating_sub(1)
            };
        }
        (x, y)
    }

    /// The position encoded by the texel at `(x, y)`, `y` counted from the
    /// visible bottom row (`(r, g, b) / 255 - 0.5`); an out-of-range texel reads
    /// as the cube's corner, which the edge handling above never asks for.
    fn texel(&self, x: usize, y: usize) -> [f32; 3] {
        let row = self.height.saturating_sub(1).saturating_sub(y);
        let base = row
            .saturating_mul(self.width)
            .saturating_add(x)
            .saturating_mul(RGBA_CHANNELS);
        let channel = |offset: usize| {
            let raw = base
                .checked_add(offset)
                .and_then(|index| self.pixels.get(index))
                .copied()
                .unwrap_or(0);
            f32::from(raw) * INV_U8_MAX - CHANNEL_CENTRE
        };
        [channel(0), channel(1), channel(2)]
    }
}

/// The texel index grid step `index` of `count` lands on across `extent` texels:
/// `floor(index / (count - 1) * extent)`, reaching `extent` itself on the last
/// step — the far edge the stitch type resolves.
fn grid_to_texel(index: usize, count: usize, extent: usize) -> usize {
    let steps = count.saturating_sub(1);
    if steps == 0 {
        return 0;
    }
    usize_from_f32_floor(f32_from_usize(index) / f32_from_usize(steps) * f32_from_usize(extent))
}

/// The surface area of a `size_s × size_t` grid, each cell counted as its two
/// triangles (Firestorm `sculptGetSurfaceArea`) — the test of whether a map
/// varies enough to make real geometry.
fn surface_area(surface: &[[f32; 3]], size_s: usize, size_t: usize) -> f32 {
    let at = |s: usize, t: usize| {
        surface
            .get(s.saturating_mul(size_t).saturating_add(t))
            .copied()
            .unwrap_or([0.0; 3])
    };
    let mut area = 0.0_f32;
    for s in 0..size_s.saturating_sub(1) {
        for t in 0..size_t.saturating_sub(1) {
            let p1 = at(s, t);
            let p2 = at(s.saturating_add(1), t);
            let p3 = at(s, t.saturating_add(1));
            let p4 = at(s.saturating_add(1), t.saturating_add(1));
            let first = length(cross(subtract(p1, p2), subtract(p1, p3)));
            let second = length(cross(subtract(p4, p2), subtract(p4, p3)));
            area += f32::midpoint(first, second);
        }
    }
    area
}

/// The visible placeholder for a surface that failed the area test: a sphere of
/// [`PLACEHOLDER_RADIUS`], its azimuth running along the path and its polar
/// angle across the profile (Firestorm `sculptGenerateSpherePlaceholder`).
fn sphere_placeholder(size_s: usize, size_t: usize) -> Vec<[f32; 3]> {
    let fraction = |index: usize, count: usize| {
        f32_from_usize(index) / f32_from_usize(count.saturating_sub(1).max(1))
    };
    let mut surface = Vec::with_capacity(size_s.saturating_mul(size_t));
    for s in 0..size_s {
        let azimuth = core::f32::consts::TAU * fraction(s, size_s);
        for t in 0..size_t {
            let polar = core::f32::consts::PI * fraction(t, size_t);
            surface.push([
                polar.sin() * azimuth.cos() * PLACEHOLDER_RADIUS,
                polar.sin() * azimuth.sin() * PLACEHOLDER_RADIUS,
                polar.cos() * PLACEHOLDER_RADIUS,
            ]);
        }
    }
    surface
}

/// The vector difference `a - b`.
fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// The cross product `a × b`.
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The Euclidean length of `v`.
fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Widen a small `usize` count to `f32`; grid and pixel counts are far below the
/// 24-bit exact-integer range, so no precision is lost.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "grid and pixel counts are small, well within f32's exact-integer range"
)]
const fn f32_from_usize(value: usize) -> f32 {
    value as f32
}

/// Floor a non-negative `f32` to `usize`, truncating like the reference's
/// `(U32)` cast; a negative or non-finite value maps to `0`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is a non-negative texel coordinate no larger than the map; its floor fits usize"
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

#[cfg(test)]
mod tests {
    use super::{
        MAX_SUBDIVISIONS, MIN_SUBDIVISIONS, PLACEHOLDER_RADIUS, mesh_resolution, tessellate,
        usize_from_f32_floor,
    };
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_prim::{PrimFace, PrimLod, PrimMesh, PrimShape};
    use sl_proto::{DiscardLevel, PrimShapeParams};
    use sl_texture::DecodedImage;

    /// The finest level, which every test that is not about level of detail
    /// tessellates at.
    const FINE: PrimLod = PrimLod::High;

    /// The side of the finest grid the tests' 64x64 maps ask for: their pixel
    /// budget (`64 * 64 / 4`) is exactly the level's, so it is
    /// [`MAX_SUBDIVISIONS`] steps each way, one more vertex than steps.
    const SIDE: usize = MAX_SUBDIVISIONS + 1;

    /// The shape a sculpt is normally given — a circle profile on a circle path
    /// with a 1.0 × 0.5 top size — as `PRIM_TYPE_SCULPT` sets it (OpenSim's
    /// `SetPrimitiveShapeParams`) and the build tool's sculpt type (circle on
    /// circle) does.
    fn sculpt_shape() -> PrimShape {
        PrimShape::from_params(&PrimShapeParams {
            path_curve: 0x20,
            profile_curve: 0x00,
            path_scale_x: 100,
            path_scale_y: 150,
            ..PrimShapeParams::default()
        })
    }

    /// A plain box's shape — square profile, line path — which a sculpt block
    /// can arrive on when nothing reset the shape.
    fn box_shape() -> PrimShape {
        PrimShape::from_params(&PrimShapeParams {
            path_curve: 0x10,
            profile_curve: 0x01,
            path_scale_x: 100,
            path_scale_y: 100,
            ..PrimShapeParams::default()
        })
    }

    /// A map of `width × height` opaque RGBA8 pixels whose RGB is `paint(x, y)`
    /// (`y` top-down, as a decoded image stores it).
    fn painted(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 3]) -> DecodedImage {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let [r, g, b] = paint(x, y);
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        DecodedImage::new(
            width,
            height,
            3,
            DiscardLevel::FULL,
            Bytes::from(pixels),
            None,
        )
    }

    /// A smooth gradient, so no two texels coincide.
    fn gradient_map(width: u32, height: u32) -> DecodedImage {
        painted(width, height, |x, y| {
            [
                u8::try_from(x.saturating_mul(255).checked_div(width).unwrap_or(0)).unwrap_or(0),
                u8::try_from(y.saturating_mul(255).checked_div(height).unwrap_or(0)).unwrap_or(0),
                u8::try_from(x.wrapping_add(y) % 256).unwrap_or(0),
            ]
        })
    }

    /// A sphere sculpt map in the **real content convention**: the north pole
    /// (`z = +0.5`, blue = 255) on the visible *top* row, longitude
    /// counter-clockwise (`+X` → `+Y`) across the columns. Real sculpt content
    /// (authored against the reference viewer) renders outward from exactly
    /// this orientation.
    fn sphere_map(width: u32, height: u32) -> DecodedImage {
        painted(width, height, |x, y| {
            let theta = core::f32::consts::PI * f32::from(u16::try_from(y).unwrap_or(0))
                / f32::from(u16::try_from(height.saturating_sub(1)).unwrap_or(1));
            let phi = core::f32::consts::TAU * f32::from(u16::try_from(x).unwrap_or(0))
                / f32::from(u16::try_from(width).unwrap_or(1));
            let channel = |value: f32| {
                let byte = ((0.5 + 0.5 * value) * 255.0).round().clamp(0.0, 255.0);
                u8::try_from(usize_from_f32_floor(byte)).unwrap_or(255)
            };
            [
                channel(theta.sin() * phi.cos()),
                channel(theta.sin() * phi.sin()),
                channel(theta.cos()),
            ]
        })
    }

    /// The signed volume enclosed by a face's triangles (`Σ p0 · (p1 × p2) / 6`):
    /// positive when the winding faces outward from the origin, negative when
    /// the surface is inside out.
    fn signed_volume(face: &PrimFace) -> f32 {
        let mut volume = 0.0_f32;
        for &[i0, i1, i2] in face.indices.as_chunks::<3>().0 {
            let p0 = position(face, i0);
            let p1 = position(face, i1);
            let p2 = position(face, i2);
            let cross = [
                p1[1] * p2[2] - p1[2] * p2[1],
                p1[2] * p2[0] - p1[0] * p2[2],
                p1[0] * p2[1] - p1[1] * p2[0],
            ];
            volume += (p0[0] * cross[0] + p0[1] * cross[1] + p0[2] * cross[2]) / 6.0;
        }
        volume
    }

    /// Vertex `index` of `face` (the origin if out of range).
    fn position(face: &PrimFace, index: u32) -> [f32; 3] {
        face.positions
            .get(usize::try_from(index).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or([0.0; 3])
    }

    /// The single face of a sculpt on its usual shape.
    fn single_face(mesh: &PrimMesh) -> &PrimFace {
        assert_eq!(
            mesh.face_count(),
            1,
            "a sculpt on its usual shape is one face"
        );
        match mesh.faces.first() {
            Some(face) => face,
            None => unreachable!("face_count of 1 guarantees a first face"),
        }
    }

    /// The vertex at path row `row`, profile column `column` of a side face
    /// `width` columns wide.
    fn vertex(face: &PrimFace, width: usize, row: usize, column: usize) -> ([f32; 3], [f32; 2]) {
        let index = row.saturating_mul(width).saturating_add(column);
        (
            face.positions.get(index).copied().unwrap_or([f32::NAN; 3]),
            face.uvs.get(index).copied().unwrap_or([f32::NAN; 2]),
        )
    }

    /// Assert every face is internally consistent: parallel vertex arrays,
    /// whole in-range triangles, unit-length normals, finite positions.
    fn assert_mesh_integrity(mesh: &PrimMesh) {
        assert!(mesh.face_count() > 0, "mesh has faces");
        for face in &mesh.faces {
            let count = face.positions.len();
            assert!(count >= 3, "face has vertices");
            assert_eq!(face.normals.len(), count, "normals parallel to positions");
            assert_eq!(face.uvs.len(), count, "uvs parallel to positions");
            assert!(!face.indices.is_empty(), "face carries triangles");
            assert_eq!(face.indices.len() % 3, 0, "indices are whole triangles");
            for &index in &face.indices {
                assert!(
                    usize::try_from(index).unwrap_or(usize::MAX) < count,
                    "index {index} within {count} vertices"
                );
            }
            for normal in &face.normals {
                let length =
                    (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
                assert!(
                    (length - 1.0).abs() < 1.0e-3,
                    "normal {normal:?} is unit length (was {length})"
                );
            }
            for position in &face.positions {
                assert!(
                    position.iter().all(|value| value.is_finite()),
                    "position {position:?} is finite"
                );
            }
        }
    }

    /// Whether two positions agree to within float noise.
    fn same(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0e-5)
    }

    /// [`mesh_resolution`] reproduces Firestorm's `sculpt_calc_mesh_resolution`.
    ///
    /// The expected `(rows, columns)` were produced by compiling that function
    /// verbatim out of `indra/llmath/llvolume.cpp` and printing its output for
    /// each map size and detail multiplier — so this pins the port to the
    /// reference, not to itself.
    #[test]
    fn resolution_matches_the_reference_viewer() {
        use PrimLod::{High, Low, Lowest, Medium};
        for (width, height, lod, expected) in [
            // A 64x64 map has exactly the finest level's vertex budget, so it is
            // the level alone that decides — the case the old fixed grid got
            // right by accident.
            (64_u32, 64_u32, Lowest, (6_usize, 6_usize)),
            (64, 64, Low, (8, 8)),
            (64, 64, Medium, (16, 16)),
            (64, 64, High, (32, 32)),
            // A bigger map cannot buy more than the level allows.
            (1024, 1024, High, (32, 32)),
            // A smaller one caps the level instead: a 32x32 map is worth 256
            // vertices, so it stops at 16x16 however close the camera gets.
            (32, 32, Medium, (16, 16)),
            (32, 32, High, (16, 16)),
            (16, 16, High, (8, 8)),
            // A non-square map splits its budget in its own aspect ratio.
            (64, 32, High, (16, 32)),
            (32, 64, High, (32, 16)),
            (256, 64, Medium, (8, 32)),
            (256, 64, Low, (4, 16)),
            // Neither axis is ever allowed below the floor.
            (8, 8, Lowest, (4, 4)),
            (8, 8, High, (4, 4)),
            // An extremely elongated map spends the budget along its long axis
            // and pins the short one to the floor — so a single axis can exceed
            // MAX_SUBDIVISIONS even though the vertex budget does not.
            (8, 512, Medium, (64, 4)),
            (512, 8, High, (4, 256)),
            // A map too small to be worth even one vertex per four pixels falls
            // back to the level's own budget, like a degenerate one.
            (1, 1, High, (32, 32)),
            // A degenerate map is sized by the level alone (and tessellates as a
            // placeholder).
            (0, 0, High, (32, 32)),
        ] {
            assert_eq!(
                mesh_resolution(width, height, lod),
                expected,
                "{width}x{height} at {lod:?}"
            );
        }
    }

    /// The level of detail bounds the grid's *vertex budget*, not either axis on
    /// its own: an extremely elongated map spends the whole budget along its long
    /// side while the short one sits on the floor. Only when the floor itself
    /// forces the issue (a map too small to fill even a 4x4 grid) does the
    /// product exceed the budget.
    #[test]
    fn resolution_spends_the_level_budget_without_degenerate_axes() {
        for lod in PrimLod::ALL {
            let sides = super::sculpt_sides(lod);
            let budget = (sides * sides).max(MIN_SUBDIVISIONS * MIN_SUBDIVISIONS);
            for (width, height) in [
                (0_u32, 0_u32),
                (1, 1),
                (8, 8),
                (8, 512),
                (512, 8),
                (64, 64),
                (2048, 2048),
            ] {
                let (rows, columns) = mesh_resolution(width, height, lod);
                assert!(
                    rows >= MIN_SUBDIVISIONS,
                    "{width}x{height} at {lod:?}: {rows} rows"
                );
                assert!(
                    columns >= MIN_SUBDIVISIONS,
                    "{width}x{height} at {lod:?}: {columns} columns"
                );
                assert!(
                    rows * columns <= budget,
                    "{width}x{height} at {lod:?}: {rows}x{columns} exceeds the {budget}-vertex budget"
                );
            }
        }
    }

    /// On its usual shape a sculpt is one closed side face over the grid the map
    /// asked for — one more vertex than steps each way, because the seam is two
    /// columns and not one — and a coarser level really is less geometry.
    #[test]
    fn the_usual_shape_is_one_face_over_the_requested_grid() {
        let map = gradient_map(64, 64);
        for (lod, steps) in [
            (PrimLod::Low, 8_usize),
            (PrimLod::Medium, 16),
            (PrimLod::High, 32),
        ] {
            for sculpt_type in [1_u8, 2, 3, 4] {
                let mesh = tessellate(&map, sculpt_type, &sculpt_shape(), lod);
                assert_mesh_integrity(&mesh);
                assert_eq!(
                    single_face(&mesh).positions.len(),
                    (steps + 1) * (steps + 1),
                    "type {sculpt_type} at {lod:?}"
                );
            }
        }
        let lowest = tessellate(&map, 1, &sculpt_shape(), PrimLod::Lowest);
        assert!(
            single_face(&lowest).positions.len() < SIDE * SIDE,
            "the lowest level is coarser than the finest"
        );
    }

    /// A non-square map lays its long side along the right axis: rows (the
    /// path) follow the map's height, columns (the profile) its width.
    #[test]
    fn a_non_square_map_lays_out_rows_by_height_and_columns_by_width() {
        let (rows, columns) = mesh_resolution(64, 32, FINE);
        assert_eq!((rows, columns), (16, 32));
        let mesh = tessellate(&gradient_map(64, 32), 3, &sculpt_shape(), FINE);
        assert_mesh_integrity(&mesh);
        assert_eq!(
            single_face(&mesh).positions.len(),
            (rows + 1) * (columns + 1)
        );
    }

    /// Each vertex reads the texel below its fraction of the map — no filtering
    /// — with rows counted from the visible bottom.
    #[test]
    fn a_vertex_reads_the_texel_below_its_fraction_of_the_map() {
        let map = gradient_map(64, 64);
        let mesh = tessellate(&map, 3, &sculpt_shape(), FINE);
        let face = single_face(&mesh);
        // Row 1, column 3 of a 33-wide grid: x = floor(3 / 32 * 64) = 6 and
        // y = floor(1 / 32 * 64) = 2 from the bottom, the top-down row 61.
        let (at, _uv) = vertex(face, SIDE, 1, 3);
        let texel = |value: u32| f32::from(u8::try_from(value).unwrap_or(0)) / 255.0 - 0.5;
        let expected = [texel(6 * 255 / 64), texel(61 * 255 / 64), texel(6 + 61)];
        assert!(same(at, expected), "{at:?} reads texel {expected:?}");
    }

    /// A seam is two vertices at one position, the first with texture U 0 and
    /// the last with U 1, so the texture runs once around. (Sharing one vertex
    /// squeezed a reversed copy of the whole texture into the last column.)
    #[test]
    fn the_seam_is_two_vertices_and_the_texture_runs_once_around() {
        let mesh = tessellate(&sphere_map(64, 64), 1, &sculpt_shape(), FINE);
        let face = single_face(&mesh);
        for row in 0..SIDE {
            let (first, first_uv) = vertex(face, SIDE, row, 0);
            let (last, last_uv) = vertex(face, SIDE, row, SIDE - 1);
            assert!(same(first, last), "row {row}: {first:?} vs {last:?}");
            assert!(first_uv[0].abs() < 1.0e-6, "row {row} starts at U 0");
            assert!((last_uv[0] - 1.0).abs() < 1.0e-6, "row {row} ends at U 1");
        }
        for (index, uv) in face.uvs.iter().enumerate() {
            let column = index % SIDE;
            let expected = f32::from(u16::try_from(column).unwrap_or(0))
                / f32::from(u16::try_from(SIDE - 1).unwrap_or(1));
            assert!(
                (uv[0] - expected).abs() < 1.0e-5,
                "vertex {index}: U {} for column {column}",
                uv[0]
            );
        }
    }

    /// A sphere pinches its first and last rows to the map's middle column, and
    /// averages each pole row's normals into one.
    #[test]
    fn a_sphere_pinches_each_pole_row_to_one_point() {
        let mesh = tessellate(&sphere_map(64, 64), 1, &sculpt_shape(), FINE);
        let face = single_face(&mesh);
        for row in [0, SIDE - 1] {
            let (pole, _uv) = vertex(face, SIDE, row, 0);
            let pole_normal = face.normals.get(row * SIDE).copied().unwrap_or_default();
            for column in 1..SIDE {
                let (at, _uv) = vertex(face, SIDE, row, column);
                assert!(same(at, pole), "row {row} column {column} is the pole");
                let normal = face
                    .normals
                    .get(row * SIDE + column)
                    .copied()
                    .unwrap_or_default();
                assert!(same(normal, pole_normal), "row {row} shares one normal");
            }
        }
    }

    /// A torus wraps its last row back onto its first.
    #[test]
    fn a_torus_wraps_its_last_row_onto_its_first() {
        let mesh = tessellate(&gradient_map(64, 64), 2, &sculpt_shape(), FINE);
        let face = single_face(&mesh);
        for column in 0..SIDE {
            let (first, _uv) = vertex(face, SIDE, 0, column);
            let (last, _uv) = vertex(face, SIDE, SIDE - 1, column);
            assert!(same(first, last), "column {column}");
        }
    }

    #[test]
    fn a_real_convention_sphere_renders_outward() {
        // The viewer-pillows-inside-out-geometry regression: a sphere sculpt
        // map in the real content convention (north pole on the visible top
        // row) must tessellate with outward-facing winding. Reading the
        // top-down map without the row flip builds this exact sphere inside out.
        let mesh = tessellate(&sphere_map(64, 64), 1, &sculpt_shape(), FINE);
        assert_mesh_integrity(&mesh);
        let volume = signed_volume(single_face(&mesh));
        assert!(volume > 0.05, "sphere faces outward (volume {volume})");
    }

    #[test]
    fn the_invert_flag_turns_the_sphere_inside_out() {
        let mesh = tessellate(&sphere_map(64, 64), 1 | 64, &sculpt_shape(), FINE);
        assert_mesh_integrity(&mesh);
        let volume = signed_volume(single_face(&mesh));
        assert!(
            volume < -0.05,
            "inverted sphere faces inward (volume {volume})"
        );
    }

    #[test]
    fn the_mirror_flag_keeps_the_sphere_outward() {
        // Mirror composes an X negation with a reversed U sweep — two
        // orientation flips, so the mirrored sphere still faces outward.
        let mesh = tessellate(&sphere_map(64, 64), 1 | 128, &sculpt_shape(), FINE);
        assert_mesh_integrity(&mesh);
        let volume = signed_volume(single_face(&mesh));
        assert!(
            volume > 0.05,
            "mirrored sphere faces outward (volume {volume})"
        );
    }

    /// The mirror flag reads each row right to left and negates X, so a
    /// mirrored vertex is its plain twin across the row with X flipped.
    #[test]
    fn the_mirror_flag_reads_rows_backwards_and_negates_x() {
        let map = gradient_map(64, 64);
        let plain = tessellate(&map, 3, &sculpt_shape(), FINE);
        let mirrored = tessellate(&map, 3 | 128, &sculpt_shape(), FINE);
        let (plain, mirrored) = (single_face(&plain), single_face(&mirrored));
        for row in 0..SIDE {
            for column in 0..SIDE {
                let (at, _uv) = vertex(mirrored, SIDE, row, column);
                let (twin, _uv) = vertex(plain, SIDE, row, SIDE - 1 - column);
                assert!(
                    same(at, [-twin[0], twin[1], twin[2]]),
                    "row {row} column {column}: {at:?} vs {twin:?}"
                );
            }
        }
    }

    /// The reference's `createSide` reverses the horizontal texture coordinate
    /// when invert XOR mirror is set (`ss = 1.f - ss`).
    #[test]
    fn the_invert_flag_reverses_the_texture_coordinate() {
        let map = gradient_map(64, 64);
        let plain = tessellate(&map, 3, &sculpt_shape(), FINE);
        let inverted = tessellate(&map, 3 | 64, &sculpt_shape(), FINE);
        let (plain, inverted) = (single_face(&plain), single_face(&inverted));
        assert_eq!(plain.uvs.len(), inverted.uvs.len());
        for (plain_uv, inverted_uv) in plain.uvs.iter().zip(&inverted.uvs) {
            assert!(
                ((1.0 - plain_uv[0]) - inverted_uv[0]).abs() < 1.0e-6,
                "U mirrored: plain {plain_uv:?} vs inverted {inverted_uv:?}"
            );
            assert!((plain_uv[1] - inverted_uv[1]).abs() < 1.0e-6, "V unchanged");
        }
    }

    /// A map with no positions in it — no pixels, or a grey one — is the
    /// reference's empty placeholder: every vertex at the origin.
    #[test]
    fn a_map_without_positions_is_an_empty_surface() {
        let empty = DecodedImage::new(0, 0, 3, DiscardLevel::FULL, Bytes::new(), None);
        let mut grey = gradient_map(64, 64);
        grey.components = 1;
        let short = DecodedImage::new(
            64,
            64,
            3,
            DiscardLevel::FULL,
            Bytes::from_static(&[10, 20, 30, 255]),
            None,
        );
        for map in [empty, grey, short] {
            let mesh = tessellate(&map, 1, &sculpt_shape(), FINE);
            assert_mesh_integrity(&mesh);
            assert!(
                single_face(&mesh)
                    .positions
                    .iter()
                    .all(|at| same(*at, [0.0; 3])),
                "{}x{} with {} components collapses to the origin",
                map.width,
                map.height,
                map.components
            );
        }
    }

    /// A map that does not vary has no area. Above the lowest level that fails
    /// the area test and shows the sphere placeholder; the lowest level skips
    /// the test and keeps the collapsed surface, as the reference does for
    /// legacy content.
    #[test]
    fn a_flat_map_shows_the_placeholder_above_the_lowest_level() {
        let flat = painted(64, 64, |_x, _y| [40, 90, 200]);
        let fine = tessellate(&flat, 1, &sculpt_shape(), FINE);
        assert_mesh_integrity(&fine);
        for at in &single_face(&fine).positions {
            let radius = (at[0] * at[0] + at[1] * at[1] + at[2] * at[2]).sqrt();
            assert!(
                (radius - PLACEHOLDER_RADIUS).abs() < 1.0e-4,
                "{at:?} is on the placeholder"
            );
        }
        let lowest = tessellate(&flat, 1, &sculpt_shape(), PrimLod::Lowest);
        let face = single_face(&lowest);
        let first = face.positions.first().copied().unwrap_or_default();
        assert!(
            face.positions.iter().all(|at| same(*at, first)),
            "the lowest level keeps the flat surface"
        );
    }

    /// The fixture divergence this port was made for: a sculpt left on a box's
    /// shape. The box's line path is two rows deep, both rows pinch to a pole,
    /// the surface has no area and becomes the placeholder — whose two rows,
    /// azimuth 0 and a full turn, are the same half circle. The box's six faces
    /// stay: four collapsed sides and two caps, and it is the caps that show,
    /// as a flat half-disc in the XZ plane.
    #[test]
    fn a_sculpt_on_a_box_is_the_references_half_disc() {
        let mesh = tessellate(&sphere_map(64, 64), 1, &box_shape(), FINE);
        assert_mesh_integrity(&mesh);
        assert_eq!(mesh.face_count(), 6, "a box's faces");
        let Some(cap) = mesh.faces.first() else {
            unreachable!("six faces")
        };
        let (mut min_z, mut max_z, mut max_x) = (f32::MAX, f32::MIN, 0.0_f32);
        for at in &cap.positions {
            assert!(at[1].abs() < 1.0e-5, "{at:?} lies in the XZ plane");
            min_z = min_z.min(at[2]);
            max_z = max_z.max(at[2]);
            max_x = max_x.max(at[0]);
        }
        assert!(
            max_z - min_z > 0.5,
            "the disc spans the placeholder's diameter"
        );
        assert!(max_x > 0.25, "and bulges out to its radius");
    }

    /// Every stitch type, with and without the flags, on a map of each aspect,
    /// gives sound geometry.
    #[test]
    fn every_stitch_type_yields_sound_geometry() {
        for map in [
            gradient_map(48, 96),
            sphere_map(64, 64),
            gradient_map(128, 32),
        ] {
            for stitch in [1_u8, 2, 3, 4] {
                for flags in [0_u8, 64, 128, 192] {
                    for shape in [sculpt_shape(), box_shape()] {
                        assert_mesh_integrity(&tessellate(&map, stitch | flags, &shape, FINE));
                    }
                }
            }
        }
    }
}
