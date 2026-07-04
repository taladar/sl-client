//! The **sculpt sweep**: reading a decoded RGB sculpt map as a displacement
//! grid and stitching it into a closed surface.
//!
//! [`tessellate`] resamples the sculpt map onto a fixed working grid
//! ([`WORKING_SUBDIVISIONS`] quad cells per side) with bilinear filtering, maps
//! each sample's `(r, g, b) / 255 - 0.5` to a vertex position in Second Life's
//! right-handed **Z-up** space, and stitches the grid per [`SculptStitch`]:
//!
//! - **plane** — an open grid, neither edge shared;
//! - **cylinder** — the U (around) seam is a single shared column;
//! - **sphere** — the U seam is shared and the top / bottom rows collapse to a
//!   single pole vertex each;
//! - **torus** — both the U and V seams are shared.
//!
//! Sharing is structural: a seam or pole is *one* vertex the surrounding quads
//! reference, never a duplicated pair, so per-vertex normals (accumulated from
//! the incident triangles) are automatically smooth across it.
//!
//! It is a faithful, idiomatic re-implementation of Firestorm
//! `indra/llmath/llvolume.cpp` — `LLVolume::sculpt` and
//! `sculptGenerateMapVertices` — reworked to the workspace's restriction lints
//! (no indexing, no `as` casts outside the bounded numeric helpers, no panics)
//! and to a self-contained resample rather than reusing the prim path / profile
//! generators. A degenerate map (zero-sized or short) falls back to a sphere
//! placeholder so the function never panics and always yields drawable geometry.

use crate::stitch::{SculptParams, SculptStitch};
use sl_prim::{PrimFace, PrimFaceId, PrimMesh};
use sl_texture::DecodedImage;

/// The number of quad cells per side of the fixed working grid the sculpt map is
/// resampled onto (matching Firestorm's highest sculpt LOD, `SCULPT_REZ_4`).
///
/// The vertex lattice is therefore `WORKING_SUBDIVISIONS + 1` points per side
/// before any seam sharing or pole collapse reduces it.
pub const WORKING_SUBDIVISIONS: usize = 32;

/// The number of bytes per pixel in a decoded [`DecodedImage`] (canonical RGBA8).
const RGBA_CHANNELS: usize = 4;

/// The inverse of the 8-bit channel range, mapping `0..=255` to `0.0..=1.0`.
const INV_U8_MAX: f32 = 1.0 / 255.0;

/// The centre offset subtracted from a normalised channel to place the origin at
/// the middle of the sculpt cube (Firestorm's `sub(0.5)`).
const CHANNEL_CENTRE: f32 = 0.5;

/// The radius of the sphere placeholder used for a degenerate map (Firestorm's
/// `sculptGenerateSpherePlaceholder` uses `0.3`).
const PLACEHOLDER_RADIUS: f32 = 0.3;

/// The squared-length threshold below which an accumulated normal is treated as
/// degenerate and replaced by a fallback up-vector.
const NORMAL_EPSILON: f32 = 1.0e-12;

/// Tessellate a decoded sculpt `map` into a single-face [`PrimMesh`], stitched
/// according to the wire `sculpt_type` byte.
///
/// The byte's low bits select the [`SculptStitch`] topology and its high bits
/// the invert / mirror flags (see [`SculptParams`]). A zero-sized or truncated
/// map falls back to a sphere placeholder.
#[must_use]
pub fn tessellate(map: &DecodedImage, sculpt_type: u8) -> PrimMesh {
    tessellate_with(map, SculptParams::from_sculpt_type(sculpt_type))
}

/// Tessellate a decoded sculpt `map` into a single-face [`PrimMesh`] using
/// already-parsed [`SculptParams`].
///
/// A zero-sized or truncated map falls back to a sphere placeholder (a sphere
/// stitch of a procedural sphere), so the result is always drawable.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `tessellate_with` reads clearly"
)]
pub fn tessellate_with(map: &DecodedImage, params: SculptParams) -> PrimMesh {
    match SculptMap::new(map) {
        Some(sculpt) => build(params.stitch, |u, v| sculpt.sample(u, v, params)),
        None => build(SculptStitch::Sphere, placeholder_position),
    }
}

/// A borrowed view over a decoded sculpt map's RGBA8 pixels, offering bilinear
/// position sampling.
struct SculptMap<'pixels> {
    /// The map width in pixels.
    width: usize,
    /// The map height in pixels.
    height: usize,
    /// The tightly packed RGBA8 pixels, row-major (`(y * width + x) * 4`).
    pixels: &'pixels [u8],
}

impl<'pixels> SculptMap<'pixels> {
    /// View `image` as a sculpt map, or `None` when it is degenerate — zero
    /// width or height, or fewer pixel bytes than its geometry requires.
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
            pixels: &image.pixels,
        })
    }

    /// The displacement position at normalised coordinates `(u, v)` (each in
    /// `0.0..=1.0`), bilinearly filtered, with the [`SculptParams`] flags
    /// applied: the U axis is reversed when [`SculptParams::reverse_u`] and the
    /// X component negated when mirrored.
    fn sample(&self, u: f32, v: f32, params: SculptParams) -> [f32; 3] {
        let sample_u = if params.reverse_u() { 1.0 - u } else { u };
        let position = self.bilinear(sample_u, v);
        if params.mirror {
            [-position[0], position[1], position[2]]
        } else {
            position
        }
    }

    /// Bilinearly sample the map at normalised `(u, v)` and map the RGB triple to
    /// a position (`(r, g, b) / 255 - 0.5`).
    fn bilinear(&self, u: f32, v: f32) -> [f32; 3] {
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

        let top = lerp3(self.texel(x0, y0), self.texel(x1, y0), tx);
        let bottom = lerp3(self.texel(x0, y1), self.texel(x1, y1), tx);
        lerp3(top, bottom, ty)
    }

    /// The position encoded by the pixel at integer `(x, y)`
    /// (`(r, g, b) / 255 - 0.5`); an out-of-range pixel reads as the cube centre.
    fn texel(&self, x: usize, y: usize) -> [f32; 3] {
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
            f32::from(raw) * INV_U8_MAX - CHANNEL_CENTRE
        };
        [channel(0), channel(1), channel(2)]
    }
}

/// Build the stitched grid for `stitch`, taking each vertex position from
/// `position` (the sculpt sampler, or the placeholder generator), evaluated at
/// the vertex's normalised `(u, v)` grid coordinates.
///
/// Seam and pole vertices are stored once and referenced by every incident quad,
/// so no seam or pole vertex is duplicated. The result is a single [`PrimFace`]
/// (face index `0`) wrapped in a [`PrimMesh`].
fn build(stitch: SculptStitch, position: impl Fn(f32, f32) -> [f32; 3]) -> PrimMesh {
    let cells = WORKING_SUBDIVISIONS;
    let mut grid = GridBuilder::new(stitch, cells);
    for row in 0..=cells {
        for col in 0..=cells {
            grid.ensure_vertex(row, col, &position);
        }
    }
    grid.stitch_indices(cells);

    let mut face = PrimFace::empty(PrimFaceId::new(0));
    face.positions = grid.positions;
    face.uvs = grid.uvs;
    face.indices = grid.indices;
    face.normals = smooth_normals(&face.positions, &face.indices);

    let mut mesh = PrimMesh::new();
    mesh.faces.push(face);
    mesh
}

/// Accumulates the stitched grid: one vertex per canonical `(row, col)` lattice
/// point (after wrap / pole aliasing), plus the triangle-list indices.
struct GridBuilder {
    /// The stitch topology, deciding which lattice points alias to a shared
    /// vertex.
    stitch: SculptStitch,
    /// The number of quad cells per side (the lattice is `cells + 1` points per
    /// side).
    cells: usize,
    /// For each canonical lattice slot (`row * (cells + 1) + col`), the stored
    /// vertex index once created.
    slots: Vec<Option<u32>>,
    /// The stored vertex positions.
    positions: Vec<[f32; 3]>,
    /// The stored per-vertex UVs, parallel to [`positions`](Self::positions).
    uvs: Vec<[f32; 2]>,
    /// The triangle-list indices into the stored vertices.
    indices: Vec<u32>,
}

impl GridBuilder {
    /// A builder for a `cells × cells`-quad grid stitched per `stitch`.
    fn new(stitch: SculptStitch, cells: usize) -> Self {
        let stride = cells.saturating_add(1);
        let slot_count = stride.saturating_mul(stride);
        Self {
            stitch,
            cells,
            slots: vec![None; slot_count],
            positions: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// The canonical `(row, col)` a lattice point maps to after seam and pole
    /// aliasing: a wrapped far edge folds back to `0`, and a pole row collapses
    /// every column to `0`.
    const fn canonical(&self, row: usize, col: usize) -> (usize, usize) {
        let is_pole_row = self.stitch.has_poles() && (row == 0 || row == self.cells);
        let wraps_far_col = self.stitch.wraps_u() && col == self.cells;
        let ccol = if is_pole_row || wraps_far_col { 0 } else { col };
        let crow = if self.stitch.wraps_v() && row == self.cells {
            0
        } else {
            row
        };
        (crow, ccol)
    }

    /// The flat slot index for a canonical `(row, col)`.
    const fn slot(&self, crow: usize, ccol: usize) -> usize {
        crow.saturating_mul(self.cells.saturating_add(1))
            .saturating_add(ccol)
    }

    /// The stored vertex index for the lattice point `(row, col)`, creating the
    /// vertex from `position` the first time its canonical slot is touched.
    fn ensure_vertex(
        &mut self,
        row: usize,
        col: usize,
        position: &impl Fn(f32, f32) -> [f32; 3],
    ) -> u32 {
        let (crow, ccol) = self.canonical(row, col);
        let slot = self.slot(crow, ccol);
        if let Some(Some(existing)) = self.slots.get(slot).copied() {
            return existing;
        }
        let is_pole_row = self.stitch.has_poles() && (crow == 0 || crow == self.cells);
        // A pole samples the middle of its map row (Firestorm's `x = width / 2`
        // pinch); an ordinary vertex reads its own column.
        let u = if is_pole_row {
            CHANNEL_CENTRE
        } else {
            f32_from_usize(ccol) / f32_from_usize(self.cells)
        };
        let v = f32_from_usize(crow) / f32_from_usize(self.cells);
        let index = u32_from_usize(self.positions.len());
        self.positions.push(position(u, v));
        self.uvs.push([u, v]);
        if let Some(cell) = self.slots.get_mut(slot) {
            *cell = Some(index);
        }
        index
    }

    /// Emit the two triangles of every quad cell, sharing the canonical vertices
    /// so seams and poles are single vertices; a triangle collapsed by a pole
    /// (two equal corners) is skipped.
    fn stitch_indices(&mut self, cells: usize) {
        for row in 0..cells {
            for col in 0..cells {
                let a = self.vertex_index(row, col);
                let b = self.vertex_index(row, col.saturating_add(1));
                let c = self.vertex_index(row.saturating_add(1), col.saturating_add(1));
                let d = self.vertex_index(row.saturating_add(1), col);
                // Winding matches sl-prim's side strip (bottom-left origin):
                // (a, c, d) then (a, b, c).
                self.push_triangle(a, c, d);
                self.push_triangle(a, b, c);
            }
        }
    }

    /// The already-stored vertex index for lattice point `(row, col)` (its
    /// canonical slot is guaranteed filled by [`ensure_vertex`](Self::ensure_vertex)).
    fn vertex_index(&self, row: usize, col: usize) -> u32 {
        let (crow, ccol) = self.canonical(row, col);
        let slot = self.slot(crow, ccol);
        self.slots.get(slot).copied().flatten().unwrap_or(0)
    }

    /// Append a triangle, skipping it when a pole collapse has made two of its
    /// corners the same vertex (a degenerate, zero-area triangle).
    fn push_triangle(&mut self, i: u32, j: u32, k: u32) {
        if i == j || j == k || k == i {
            return;
        }
        self.indices.extend_from_slice(&[i, j, k]);
    }
}

/// A procedural sphere position for the degenerate-map placeholder, evaluated at
/// normalised grid coordinates `(u, v)` (Firestorm's
/// `sculptGenerateSpherePlaceholder`).
fn placeholder_position(u: f32, v: f32) -> [f32; 3] {
    let theta = core::f32::consts::PI * v;
    let phi = core::f32::consts::TAU * u;
    [
        theta.sin() * phi.cos() * PLACEHOLDER_RADIUS,
        theta.sin() * phi.sin() * PLACEHOLDER_RADIUS,
        theta.cos() * PLACEHOLDER_RADIUS,
    ]
}

/// Per-vertex smooth normals: sum each incident triangle's face normal, then
/// normalise (a degenerate near-zero normal becomes an up-vector). Shared seam /
/// pole vertices are single entries, so this is smooth across them without any
/// extra seam wrapping.
fn smooth_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0_f32; 3]; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let [i0, i1, i2] = match triangle {
            [i0, i1, i2] => [
                usize_from_u32(*i0),
                usize_from_u32(*i1),
                usize_from_u32(*i2),
            ],
            _short => continue,
        };
        let (Some(p0), Some(p1), Some(p2)) = (
            positions.get(i0).copied(),
            positions.get(i1).copied(),
            positions.get(i2).copied(),
        ) else {
            continue;
        };
        let face_normal = cross(subtract(p1, p0), subtract(p2, p0));
        add_normal(&mut normals, i0, face_normal);
        add_normal(&mut normals, i1, face_normal);
        add_normal(&mut normals, i2, face_normal);
    }
    for normal in &mut normals {
        if dot(*normal, *normal) > NORMAL_EPSILON {
            *normal = normalize(*normal);
        } else {
            *normal = [0.0, 0.0, 1.0];
        }
    }
    normals
}

/// Add `value` into the accumulated normal at `index` (a no-op if out of range).
fn add_normal(normals: &mut [[f32; 3]], index: usize, value: [f32; 3]) {
    if let Some(slot) = normals.get_mut(index) {
        slot[0] += value[0];
        slot[1] += value[1];
        slot[2] += value[2];
    }
}

/// Linearly interpolate two 3D points by `t` (`a + (b - a) * t`).
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
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

/// The dot product `a · b`.
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The unit vector in the direction of `v`; the caller guarantees `v` is
/// non-degenerate.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = dot(v, v).sqrt();
    if length > 0.0 {
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        v
    }
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

/// Widen a `u32` index to `usize` (lossless on every supported target).
fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// Narrow a `usize` vertex index to `u32` for the index buffer; sculpt vertex
/// counts are far below `u32::MAX`, so a saturating conversion never loses one.
fn u32_from_usize(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{WORKING_SUBDIVISIONS, tessellate, tessellate_with};
    use crate::stitch::{SculptParams, SculptStitch};
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_prim::PrimMesh;
    use sl_proto::DiscardLevel;
    use sl_texture::DecodedImage;

    /// A synthetic sculpt map: `width × height` RGBA8 pixels whose RGB is a
    /// smooth gradient (so no two grid samples coincide), fully opaque.
    fn gradient_map(width: u32, height: u32) -> DecodedImage {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let r = u8::try_from(x.saturating_mul(255).checked_div(width).unwrap_or(0))
                    .unwrap_or(0);
                let g = u8::try_from(y.saturating_mul(255).checked_div(height).unwrap_or(0))
                    .unwrap_or(0);
                let b = u8::try_from(x.wrapping_add(y) % 256).unwrap_or(0);
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        DecodedImage {
            width,
            height,
            components: 3,
            discard_level: DiscardLevel::FULL,
            pixels: Bytes::from(pixels),
        }
    }

    /// The number of quad cells per side used by the working grid.
    const N: usize = WORKING_SUBDIVISIONS;

    /// The single face of a tessellated sculpt (there is always exactly one).
    fn single_face(mesh: &PrimMesh) -> &sl_prim::PrimFace {
        assert_eq!(mesh.face_count(), 1, "a sculpt is a single face");
        match mesh.faces.first() {
            Some(face) => face,
            None => unreachable!("face_count of 1 guarantees a first face"),
        }
    }

    /// The `(min, max)` of a face's X positions.
    fn x_bounds(face: &sl_prim::PrimFace) -> (f32, f32) {
        face.positions
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                (lo.min(p[0]), hi.max(p[0]))
            })
    }

    /// Assert a face is internally consistent: parallel vertex arrays, whole
    /// in-range triangles, and unit-length normals.
    fn assert_face_integrity(mesh: &PrimMesh) {
        let face = single_face(mesh);
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
        for triangle in face.indices.chunks_exact(3) {
            if let [i, j, k] = triangle {
                assert!(i != j && j != k && k != i, "no degenerate triangle");
            }
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
            for value in position {
                assert!(value.is_finite(), "position {position:?} is finite");
            }
        }
    }

    #[test]
    fn plane_shares_no_edges() {
        // Sculpt type 3 = plane. An open grid has the full lattice of vertices.
        let mesh = tessellate(&gradient_map(64, 64), 3);
        assert_face_integrity(&mesh);
        assert_eq!(mesh.vertex_count(), (N + 1) * (N + 1));
    }

    #[test]
    fn cylinder_shares_the_u_seam() {
        // Sculpt type 4 = cylinder: the U seam folds the far column onto the
        // first, so one column fewer than a plane.
        let mesh = tessellate(&gradient_map(64, 64), 4);
        assert_face_integrity(&mesh);
        assert_eq!(mesh.vertex_count(), N * (N + 1));
        assert!(mesh.vertex_count() < (N + 1) * (N + 1), "seam is shared");
    }

    #[test]
    fn sphere_shares_the_seam_and_collapses_poles() {
        // Sculpt type 1 = sphere: U seam shared plus two single pole vertices.
        let mesh = tessellate(&gradient_map(64, 64), 1);
        assert_face_integrity(&mesh);
        // Two poles + (N - 1) interior rows of N columns each.
        assert_eq!(mesh.vertex_count(), N * (N - 1) + 2);
    }

    #[test]
    fn torus_shares_both_seams() {
        // Sculpt type 2 = torus: both seams folded, so an N × N lattice.
        let mesh = tessellate(&gradient_map(64, 64), 2);
        assert_face_integrity(&mesh);
        assert_eq!(mesh.vertex_count(), N * N);
        assert!(mesh.vertex_count() < N * (N + 1), "both seams are shared");
    }

    #[test]
    fn stitch_types_produce_distinct_vertex_counts() {
        let map = gradient_map(64, 64);
        let plane = tessellate(&map, 3).vertex_count();
        let cylinder = tessellate(&map, 4).vertex_count();
        let sphere = tessellate(&map, 1).vertex_count();
        let torus = tessellate(&map, 2).vertex_count();
        // Each extra shared edge / pole removes vertices.
        assert!(plane > cylinder);
        assert!(cylinder > torus);
        assert!(torus > sphere);
    }

    #[test]
    fn degenerate_map_falls_back_to_a_sphere_placeholder() {
        // A zero-sized map cannot be sampled; the placeholder is a sphere.
        let empty = DecodedImage {
            width: 0,
            height: 0,
            components: 3,
            discard_level: DiscardLevel::FULL,
            pixels: Bytes::new(),
        };
        let mesh = tessellate(&empty, 3);
        assert_face_integrity(&mesh);
        // Sphere topology regardless of the requested (plane) stitch.
        assert_eq!(mesh.vertex_count(), N * (N - 1) + 2);
    }

    #[test]
    fn truncated_map_falls_back_without_panicking() {
        // Claims 64×64 but carries a single pixel: too short, so placeholder.
        let short = DecodedImage {
            width: 64,
            height: 64,
            components: 3,
            discard_level: DiscardLevel::FULL,
            pixels: Bytes::from_static(&[10, 20, 30, 255]),
        };
        let mesh = tessellate(&short, 2);
        assert_face_integrity(&mesh);
        assert_eq!(mesh.vertex_count(), N * (N - 1) + 2);
    }

    #[test]
    fn mirror_flag_negates_x_without_changing_topology() {
        let map = gradient_map(64, 64);
        let plain = tessellate_with(
            &map,
            SculptParams {
                stitch: SculptStitch::Plane,
                invert: false,
                mirror: false,
            },
        );
        let mirrored = tessellate_with(
            &map,
            SculptParams {
                stitch: SculptStitch::Plane,
                invert: false,
                mirror: true,
            },
        );
        assert_eq!(plain.vertex_count(), mirrored.vertex_count());
        assert_face_integrity(&mirrored);
        // Mirroring negates X, so the mirrored X range is the plain X range
        // reflected through zero: mirrored.min == -plain.max, mirrored.max ==
        // -plain.min.
        let (plain_min, plain_max) = x_bounds(single_face(&plain));
        let (mirror_min, mirror_max) = x_bounds(single_face(&mirrored));
        assert!((mirror_min + plain_max).abs() < 1.0e-4, "min reflects max");
        assert!((mirror_max + plain_min).abs() < 1.0e-4, "max reflects min");
    }

    #[test]
    fn every_stitch_type_yields_finite_normalized_geometry() {
        let map = gradient_map(48, 96);
        for sculpt_type in [1_u8, 2, 3, 4] {
            assert_face_integrity(&tessellate(&map, sculpt_type));
        }
    }
}
