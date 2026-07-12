//! The **sweep**: dragging the 2D [`profile`](crate::profile) ring along the
//! 3D extrusion [`path`](crate::path) and assembling per-face geometry.
//!
//! This is the join of the two halves. [`tessellate`] generates the path and
//! the profile for a [`PrimShape`] at a [`PrimLod`], builds the swept vertex
//! grid (each profile point placed into each path frame), and then walks the
//! profile's semantic [`ProfileFace`] list, emitting one [`PrimFace`] per face:
//!
//! - **side** faces (`build_side`) are a `count × path.total` grid strip —
//!   positions from the swept grid, per-vertex texture coordinates (the profile
//!   point's sweep parameter across, the path frame's `tex_t` along),
//!   two-triangle-per-cell indices, and accumulated-then-normalized normals with
//!   the reference viewer's seam/pole normal wrapping for closed rings;
//! - **cap** faces (`build_cap`) are the top / bottom polygons — a triangle
//!   **fan** around a computed centre vertex, planar UVs, and one flat normal.
//!
//! It is a faithful, idiomatic re-implementation of Firestorm
//! `indra/llmath/llvolume.cpp` — `LLVolume::generate` (the swept grid),
//! `LLVolumeFace::createSide`, and `LLVolumeFace::createCap` — reworked to the
//! workspace's restriction lints (no indexing, no `as` casts outside the bounded
//! numeric helpers, no panics). The i-th profile face becomes the i-th
//! [`PrimFace`], and that sequential index is the Linden
//! [`PrimFaceId`] the simulator textures the face from
//! (`TextureEntry.faces[i]`).
//!
//! Cap triangulation follows the reference viewer's three shapes:
//!
//! - a plain **box** cap — a solid, uncut, full-path square profile on a line
//!   path — is a bilinear vertex **grid** (`build_uncut_cube_cap`,
//!   `LLVolumeFace::createUnCutCubeCap`), i.e. a proper two-triangle quad. A
//!   centre-vertex fan over the same four corners triangulates the square into
//!   four triangles meeting at the middle, so a real texture shows the fan's
//!   diagonals as an **X / cross through the centre** (a visible artefact on
//!   every cube face); the two-triangle grid matches the reference viewer and
//!   removes it.
//! - any other **solid** cap (round profiles, cut / tapered boxes) is a
//!   centre-vertex triangle **fan** (`build_cap`, `createCap`'s fan branch);
//!   the reference viewer fans these too, so the shading matches.
//! - a **hollow** cap is an **annulus** (`build_hollow_cap`) — an area-based
//!   ear split that bridges the outer and inner rings (`createCap`'s hollow
//!   branch), so the hole stays open and every triangle winds outward. A plain
//!   centre fan over a hollow prim's `outer ++ reversed-inner` ring would wind
//!   its inner-ring half backwards (back-face culled → a see-through cap), so a
//!   hollow prim needs the annulus.
//!
//! One deliberate MVP simplification remains relative to the reference viewer:
//!
//! - **Inner-side faces are a plain strip.** The reference viewer doubles a flat
//!   hollow inner wall's column count so each inner segment carries its own flat
//!   normal; here the inner wall is a single smoothed strip. Geometry is
//!   identical; only the shading of a hollow prism's inner corners differs.

use crate::PrimLod;
use crate::geometry::{PrimFace, PrimFaceId, PrimMesh};
use crate::path::Path;
use crate::profile::{Profile, ProfileFace, ProfileFaceId, ProfilePoint};
use crate::shape::{PathCurve, PrimShape, ProfileCurve};

/// The detail multiplier applied to derive the per-edge split count (Firestorm
/// `split = (S32)(mDetail * 0.66f)`).
const SPLIT_DETAIL: f32 = 0.66;

/// The centre offset added to a cap's planar texture coordinates (Firestorm's
/// `+ 0.5f`), mapping the unit-radius profile into the `[0, 1]` UV square.
const CAP_UV_CENTRE: f32 = 0.5;

/// The squared-length threshold below which two swept points are treated as
/// coincident — a converged pole (Firestorm's `0.000001f`).
const CONVERGE_EPSILON: f32 = 0.000_001;

/// The squared-length threshold below which an accumulated normal is treated as
/// degenerate and replaced by a fallback up-vector.
const NORMAL_EPSILON: f32 = 1.0e-12;

/// Tessellate `shape` into a [`PrimMesh`] at level of detail `lod`.
///
/// Generates the extrusion [`Path`] and the [`Profile`] ring (both at the
/// reference viewer's per-edge split for `lod`), sweeps the profile along the
/// path into a vertex grid, and emits one [`PrimFace`] per semantic profile
/// face, in Linden face order. A face that cannot be built (a degenerate strip
/// or an empty ring) becomes an empty [`PrimFace`] so the face indices stay
/// aligned with the simulator's texture-entry slots.
#[must_use]
pub fn tessellate(shape: &PrimShape, lod: PrimLod) -> PrimMesh {
    let split = split_for(shape, lod);
    let path = Path::generate(shape, lod, split);
    tessellate_with_path(shape, lod, &path)
}

/// Tessellate `shape` into a [`PrimMesh`] sweeping its profile along a
/// **caller-supplied** [`Path`], instead of the one [`tessellate`] generates from
/// the shape.
///
/// This is the join point the flexible-prim ([`crate::flexi`]) simulation drives:
/// its chain solver deforms the extrusion path each frame, and this re-sweeps the
/// prim's profile along that deformed path — the reference viewer's
/// `LLVolumeImplFlexible::doFlexibleUpdate` overwriting `LLPath::mPath` before the
/// volume regenerates its faces. The profile ring (and its per-edge split) is
/// still derived from `shape` at `lod`; only the path is substituted, so the face
/// count, order, and [`PrimFaceId`] slots match [`tessellate`] for the same shape
/// and a path of the same length / open-ness.
#[must_use]
pub fn tessellate_with_path(shape: &PrimShape, lod: PrimLod, path: &Path) -> PrimMesh {
    let split = split_for(shape, lod);
    let profile = Profile::generate(shape, lod, path.is_open(), split);
    let grid = SweptGrid::new(path, &profile);

    let mut mesh = PrimMesh::new();
    for (index, face) in profile.faces.iter().enumerate() {
        let face_id = PrimFaceId::new(u16_from_usize(index));
        let prim_face = if face.cap {
            if profile.total_out > 0 {
                build_hollow_cap(&grid, &profile, face, face_id)
            } else if is_uncut_cube(shape) {
                build_uncut_cube_cap(&grid, &profile, face, face_id)
            } else {
                build_cap(&grid, &profile, face, face_id)
            }
        } else {
            build_side(&grid, &profile, path, shape, face, face_id)
        };
        mesh.faces.push(prim_face);
    }
    mesh
}

/// The per-edge split count for `shape` at `lod` (Firestorm `LLVolume::generate`):
/// `floor(detail * 0.66)`, except a straight-sided profile on a scaled line path
/// takes no split (its flat sides gain nothing from edge subdivision).
fn split_for(shape: &PrimShape, lod: PrimLod) -> u32 {
    let straight_profile = matches!(
        shape.profile_curve,
        ProfileCurve::Square
            | ProfileCurve::IsoTriangle
            | ProfileCurve::EqualTriangle
            | ProfileCurve::RightTriangle
    );
    let scaled = (shape.path_scale_x - 1.0).abs() > f32::EPSILON
        || (shape.path_scale_y - 1.0).abs() > f32::EPSILON;
    if shape.path_curve == PathCurve::Line && scaled && straight_profile {
        return 0;
    }
    floor_to_u32(lod.detail() * SPLIT_DETAIL)
}

/// The swept vertex grid: every profile point placed into every path frame.
///
/// Vertices are stored row-major — path frame (row) `t` outer, profile point
/// (column) `s` inner — so vertex `(s, t)` is at flat index `t * max_s + s`.
/// This mirrors Firestorm's `mMesh` (`sizeT * sizeS` entries).
struct SweptGrid {
    /// The swept positions, row-major (`t * max_s + s`), in the prim's local
    /// right-handed Z-up space.
    positions: Vec<[f32; 3]>,
    /// The profile point count per row (Firestorm `getProfile().getTotal()`).
    max_s: usize,
    /// The path frame count (Firestorm `getPath().mPath.size()`).
    max_t: usize,
}

impl SweptGrid {
    /// Build the grid by placing each [`Profile`] point into each [`Path`] frame
    /// (Firestorm `LLVolume::generate`'s `rotate(scale ⊙ profile) + offset`).
    fn new(path: &Path, profile: &Profile) -> Self {
        let max_s = profile.points.len();
        let max_t = path.points.len();
        let mut positions = Vec::with_capacity(max_s.saturating_mul(max_t));
        for frame in &path.points {
            for point in &profile.points {
                positions.push(frame.place(point.position));
            }
        }
        Self {
            positions,
            max_s,
            max_t,
        }
    }

    /// The swept position at profile column `col`, path row `row`. A column at
    /// or past `max_s` wraps back to the ring start on the same row (Firestorm's
    /// `mBeginS + s >= max_s` wrap), closing a full-ring side face.
    fn position(&self, col: usize, row: usize) -> [f32; 3] {
        let col = if col >= self.max_s {
            col.saturating_sub(self.max_s)
        } else {
            col
        };
        let index = row.saturating_mul(self.max_s).saturating_add(col);
        self.positions.get(index).copied().unwrap_or([0.0; 3])
    }
}

/// Build one **side** face: the `face.count × path.total` grid strip spanning
/// the profile face's ring slice (Firestorm `LLVolumeFace::createSide`).
///
/// Positions come from the swept grid, texture coordinates from the profile
/// sweep parameter (across) and the path `tex_t` (along), indices tessellate
/// each grid cell into two triangles, and normals are accumulated per triangle,
/// wrapped across closed seams / poles, then normalized.
fn build_side(
    grid: &SweptGrid,
    profile: &Profile,
    path: &Path,
    shape: &PrimShape,
    face: &ProfileFace,
    face_id: PrimFaceId,
) -> PrimFace {
    let num_s = face.count;
    let num_t = grid.max_t;
    if num_s < 2 || num_t < 2 {
        return PrimFace::empty(face_id);
    }

    let flat = face.flat;
    let is_end =
        face.face_id == ProfileFaceId::PROFILE_BEGIN || face.face_id == ProfileFaceId::PROFILE_END;
    let begin_stex = profile
        .points
        .get(face.index)
        .map_or(0.0, |point| point.u.floor());

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(num_s.saturating_mul(num_t));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(num_s.saturating_mul(num_t));
    for t in 0..num_t {
        let tex_t = path.points.get(t).map_or(0.0, |frame| frame.tex_t);
        for s in 0..num_s {
            let col = face.index.saturating_add(s);
            positions.push(grid.position(col, t));
            uvs.push([side_u(profile, col, s, flat, is_end, begin_stex), tex_t]);
        }
    }

    let indices = side_indices(num_s, num_t);
    let mut normals = accumulate_normals(&positions, &indices);
    wrap_side_normals(&mut normals, grid, num_s, num_t, path, profile, shape);
    normalize_all(&mut normals);

    PrimFace {
        positions,
        normals,
        uvs,
        indices,
        face_id,
    }
}

/// The horizontal (U) texture coordinate for side vertex `s` at ring column
/// `col` (Firestorm `createSide`'s `ss`): the profile edge faces run a flat
/// `0 → 1`, an ordinary face reads the profile point's sweep parameter, and a
/// flat face is shifted to start at zero (`- floor(begin.u)`).
fn side_u(
    profile: &Profile,
    col: usize,
    s: usize,
    flat: bool,
    is_end: bool,
    begin_stex: f32,
) -> f32 {
    if is_end {
        return if s > 0 { 1.0 } else { 0.0 };
    }
    match profile.points.get(col) {
        None if flat => 1.0 - begin_stex,
        None => 1.0,
        Some(point) if flat => point.u - begin_stex,
        Some(point) => point.u,
    }
}

/// The two-triangles-per-cell index list for a `num_s × num_t` grid strip
/// (Firestorm `createSide`'s index loop). Winding matches the reference viewer:
/// bottom-left / top-right / top-left, then bottom-left / bottom-right /
/// top-right.
fn side_indices(num_s: usize, num_t: usize) -> Vec<u32> {
    let cells = num_s
        .saturating_sub(1)
        .saturating_mul(num_t.saturating_sub(1));
    let mut indices = Vec::with_capacity(cells.saturating_mul(6));
    for t in 0..num_t.saturating_sub(1) {
        for s in 0..num_s.saturating_sub(1) {
            let bottom_left = u32_from_usize(grid_index(s, t, num_s));
            let bottom_right = u32_from_usize(grid_index(s.saturating_add(1), t, num_s));
            let top_left = u32_from_usize(grid_index(s, t.saturating_add(1), num_s));
            let top_right =
                u32_from_usize(grid_index(s.saturating_add(1), t.saturating_add(1), num_s));
            indices.extend_from_slice(&[
                bottom_left,
                top_right,
                top_left,
                bottom_left,
                bottom_right,
                top_right,
            ]);
        }
    }
    indices
}

/// Wrap a side face's accumulated normals across its closed seams and poles
/// before normalizing (Firestorm `createSide`'s non-sculpt stitching): a closed
/// path shares the first / last ring's normals, a closed profile shares the
/// ring seam's, and a half-circle profile on a circular path collapses its
/// converged poles to the axis.
fn wrap_side_normals(
    normals: &mut [[f32; 3]],
    grid: &SweptGrid,
    num_s: usize,
    num_t: usize,
    path: &Path,
    profile: &Profile,
    shape: &PrimShape,
) {
    let (bottom_converges, top_converges) = pole_convergence(grid, num_s, num_t);

    if !path.is_open() {
        // Share normals between the first and last path ring (closed loop).
        for s in 0..num_s {
            let first = grid_index(s, 0, num_s);
            let last = grid_index(s, num_t.saturating_sub(1), num_s);
            share_normals(normals, first, last);
        }
    }

    if !profile.is_open() && !bottom_converges {
        // Share normals across the profile ring seam (closed ring).
        for t in 0..num_t {
            let start = grid_index(0, t, num_s);
            let end = grid_index(num_s.saturating_sub(1), t, num_s);
            share_normals(normals, start, end);
        }
    }

    // A half-circle profile swept on a circular path collapses to an axis at the
    // converged poles (the reference viewer's only pole special-case); give
    // those columns the axis normal directly.
    let half_on_circle =
        shape.path_curve == PathCurve::Circle && shape.profile_curve == ProfileCurve::HalfCircle;
    if half_on_circle {
        if bottom_converges {
            for t in 0..num_t {
                set_normal(normals, grid_index(0, t, num_s), [1.0, 0.0, 0.0]);
            }
        }
        if top_converges {
            for t in 0..num_t {
                set_normal(
                    normals,
                    grid_index(num_s.saturating_sub(1), t, num_s),
                    [-1.0, 0.0, 0.0],
                );
            }
        }
    }
}

/// Whether the first (`bottom`) and last (`top`) profile columns of a side face
/// converge to a single swept point across the path (a pole), by comparing the
/// first path ring's edge vertices to the penultimate ring's (Firestorm's
/// `s_bottom_converges` / `s_top_converges`).
fn pole_convergence(grid: &SweptGrid, num_s: usize, num_t: usize) -> (bool, bool) {
    if num_t < 2 {
        return (false, false);
    }
    let penultimate = num_t.saturating_sub(2);
    let first_bottom = grid.position(0, 0);
    let far_bottom = grid.position(0, penultimate);
    let first_top = grid.position(num_s.saturating_sub(1), 0);
    let far_top = grid.position(num_s.saturating_sub(1), penultimate);
    (
        squared_distance(first_bottom, far_bottom) < CONVERGE_EPSILON,
        squared_distance(first_top, far_top) < CONVERGE_EPSILON,
    )
}

/// Whether `shape` is a plain **box** — a solid, uncut, full-path square profile
/// on a line path — the case the reference viewer caps with its optimised
/// two-triangle grid (`createCap`'s `createUnCutCubeCap` guard) instead of a
/// centre fan.
fn is_uncut_cube(shape: &PrimShape) -> bool {
    shape.profile_curve == ProfileCurve::Square
        && shape.path_curve == PathCurve::Line
        && !shape.is_hollow()
        && !shape.is_profile_cut()
        && !shape.is_path_cut()
}

/// Build a plain **box** cap as a bilinear vertex grid — a proper two-triangle
/// quad — rather than a centre fan (Firestorm `LLVolumeFace::createUnCutCubeCap`).
///
/// The four square corners are read from the swept grid at the cap's path frame,
/// their planar texture coordinates from the profile (the top face swaps its `U`
/// axis, mirroring the reference viewer), and a `(grid_size + 1)²` vertex grid is
/// filled by planar interpolation between three corners so a split (twisted) box
/// keeps one quad per cell. This replaces the centre fan whose four triangles
/// would meet at the face middle and draw an X / cross through any real texture.
fn build_uncut_cube_cap(
    grid: &SweptGrid,
    profile: &Profile,
    face: &ProfileFace,
    face_id: PrimFaceId,
) -> PrimFace {
    let ring_len = profile.points.len();
    let grid_size = ring_len.saturating_sub(1).checked_div(4).unwrap_or(0);
    if grid_size == 0 {
        // Not the expected 4·n+1 square ring; fall back to the fan.
        return build_cap(grid, profile, face, face_id);
    }
    let top = face.face_id == ProfileFaceId::PATH_BEGIN;
    let row = if top { grid.max_t.saturating_sub(1) } else { 0 };

    // The four square corners: positions from the swept grid, planar UVs from the
    // profile (Firestorm's `profile[i].x + 0.5`, `0.5 - profile[i].y`).
    let mut corner_pos = [[0.0_f32; 3]; 4];
    let mut corner_uv = [[0.0_f32; 2]; 4];
    for (corner, slot) in corner_pos.iter_mut().zip(corner_uv.iter_mut()).enumerate() {
        let col = grid_size.saturating_mul(corner);
        *slot.0 = grid.position(col, row);
        let position = profile
            .points
            .get(col)
            .map_or([0.0, 0.0], |point| point.position);
        *slot.1 = [position[0] + CAP_UV_CENTRE, CAP_UV_CENTRE - position[1]];
    }

    // One flat normal from the first three corner positions (before the UV swap).
    let mut normal = cross(
        subtract(corner_pos[1], corner_pos[0]),
        subtract(corner_pos[2], corner_pos[1]),
    );
    if dot(normal, normal) > NORMAL_EPSILON {
        normal = normalize(normal);
    } else {
        normal = if top {
            [0.0, 0.0, 1.0]
        } else {
            [0.0, 0.0, -1.0]
        };
    }
    if !top {
        normal = [-normal[0], -normal[1], -normal[2]];
    } else {
        // The top face mirrors its U axis: swap the corner UVs 0↔3 and 1↔2.
        corner_uv.swap(0, 3);
        corner_uv.swap(1, 2);
    }

    // Fill the (grid_size + 1)² vertex grid by planar interpolation from corner 0
    // along corners 1 (U) and 3 (V), in the reference viewer's `gx` outer / `gy`
    // inner push order so the index formula below lines up.
    let span = f32_from_usize(grid_size);
    let side = grid_size.saturating_add(1);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(side.saturating_mul(side));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(side.saturating_mul(side));
    for gx in 0..side {
        for gy in 0..side {
            let c01 = f32_from_usize(gx) / span;
            let c02 = f32_from_usize(gy) / span;
            positions.push(lerp_planar3(
                corner_pos[0],
                corner_pos[1],
                corner_pos[3],
                c01,
                c02,
            ));
            uvs.push(lerp_planar2(
                corner_uv[0],
                corner_uv[1],
                corner_uv[3],
                c01,
                c02,
            ));
        }
    }

    let indices = uncut_cube_indices(grid_size, top);
    let normals = vec![normal; positions.len()];

    PrimFace {
        positions,
        normals,
        uvs,
        indices,
        face_id,
    }
}

/// The two-triangles-per-cell index list for a box cap's `(grid_size + 1)²`
/// vertex grid (Firestorm `createUnCutCubeCap`'s index loop). The underside emits
/// the six indices in order; the top reverses them so its single flat normal
/// faces up.
fn uncut_cube_indices(grid_size: usize, top: bool) -> Vec<u32> {
    let side = grid_size.saturating_add(1);
    // The six cell corners relative to the cell's base vertex (`gy·side + gx`).
    let offsets = [
        0,
        1,
        side.saturating_add(1),
        side.saturating_add(1),
        side,
        0,
    ];
    let mut indices = Vec::with_capacity(grid_size.saturating_mul(grid_size).saturating_mul(6));
    for gx in 0..grid_size {
        for gy in 0..grid_size {
            let base = gy.saturating_mul(side).saturating_add(gx);
            if top {
                for offset in offsets.iter().rev() {
                    indices.push(u32_from_usize(base.saturating_add(*offset)));
                }
            } else {
                for offset in &offsets {
                    indices.push(u32_from_usize(base.saturating_add(*offset)));
                }
            }
        }
    }
    indices
}

/// Planar interpolation of a 3D position: `v0 + c01·(v1 − v0) + c02·(v2 − v0)`
/// (Firestorm `LerpPlanarVertex`).
fn lerp_planar3(v0: [f32; 3], v1: [f32; 3], v2: [f32; 3], c01: f32, c02: f32) -> [f32; 3] {
    [
        v0[0] + (v1[0] - v0[0]) * c01 + (v2[0] - v0[0]) * c02,
        v0[1] + (v1[1] - v0[1]) * c01 + (v2[1] - v0[1]) * c02,
        v0[2] + (v1[2] - v0[2]) * c01 + (v2[2] - v0[2]) * c02,
    ]
}

/// Planar interpolation of a 2D texture coordinate (the UV half of
/// [`lerp_planar3`]).
fn lerp_planar2(v0: [f32; 2], v1: [f32; 2], v2: [f32; 2], c01: f32, c02: f32) -> [f32; 2] {
    [
        v0[0] + (v1[0] - v0[0]) * c01 + (v2[0] - v0[0]) * c02,
        v0[1] + (v1[1] - v0[1]) * c01 + (v2[1] - v0[1]) * c02,
    ]
}

/// Build one **cap** face: a triangle fan around a computed centre vertex over
/// the profile ring at the path's begin or end frame (Firestorm
/// `LLVolumeFace::createCap`, fan branch).
///
/// The begin cap (`LL_FACE_PATH_BEGIN`) sits on the last path ring, the end cap
/// (`LL_FACE_PATH_END`) on the first; texture coordinates are the planar profile
/// position centred in the UV square (mirrored for the underside), and one flat
/// normal is applied to every vertex.
fn build_cap(
    grid: &SweptGrid,
    profile: &Profile,
    face: &ProfileFace,
    face_id: PrimFaceId,
) -> PrimFace {
    let ring_count = profile.points.len();
    if ring_count < 3 {
        return PrimFace::empty(face_id);
    }
    let top = face.face_id == ProfileFaceId::PATH_BEGIN;
    let row = if top { grid.max_t.saturating_sub(1) } else { 0 };

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(ring_count.saturating_add(1));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(ring_count.saturating_add(1));
    for (col, point) in profile.points.iter().enumerate() {
        positions.push(grid.position(col, row));
        uvs.push(cap_uv(point.position, top));
    }

    let centre = bounds_centre(&positions);
    let centre_uv = bounds_centre_2d(&uvs);
    positions.push(centre);
    uvs.push(centre_uv);
    let centre_index = u32_from_usize(ring_count);

    let indices = cap_indices(ring_count, centre_index, top);
    let normal = cap_normal(&positions, &indices, top);
    let normals = vec![normal; positions.len()];

    PrimFace {
        positions,
        normals,
        uvs,
        indices,
        face_id,
    }
}

/// Build one **hollow cap** face: an annulus bridging the outer and inner
/// profile rings at the path's begin or end frame (Firestorm
/// `LLVolumeFace::createCap`, hollow branch).
///
/// Unlike the solid [`build_cap`] there is no centre vertex; the ring points are
/// triangulated directly by walking one pointer forward from the outer-ring
/// start and one backward from the inner-ring start, choosing at each step the
/// non-back-facing triangle (`hollow_cap_indices`). This keeps the hole open and
/// winds every triangle the same way, where a plain centre fan over the
/// `outer ++ reversed-inner` ring would flip its inner-ring half. One flat
/// normal (from the first triangle) is applied to every vertex.
fn build_hollow_cap(
    grid: &SweptGrid,
    profile: &Profile,
    face: &ProfileFace,
    face_id: PrimFaceId,
) -> PrimFace {
    let ring_count = profile.points.len();
    if ring_count < 3 {
        return PrimFace::empty(face_id);
    }
    let top = face.face_id == ProfileFaceId::PATH_BEGIN;
    let row = if top { grid.max_t.saturating_sub(1) } else { 0 };

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(ring_count);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(ring_count);
    for (col, point) in profile.points.iter().enumerate() {
        positions.push(grid.position(col, row));
        uvs.push(cap_uv(point.position, top));
    }

    let indices = hollow_cap_indices(&profile.points, top);
    let normal = cap_normal(&positions, &indices, top);
    let normals = vec![normal; positions.len()];

    PrimFace {
        positions,
        normals,
        uvs,
        indices,
        face_id,
    }
}

/// The annulus triangle indices bridging a hollow cap's outer and inner rings
/// (Firestorm `createCap`'s hollow ear-split). `pt1` walks forward from the
/// outer-ring start, `pt2` backward from the inner-ring start (the reversed
/// inner ring places its begin at the ring end), and each step emits whichever
/// of the two candidate triangles is not back-facing — decided from the
/// **un-transformed** profile 2D positions, exactly as the reference viewer. The
/// underside (`!top`) winds the opposite way so its flat normal faces down.
fn hollow_cap_indices(ring: &[ProfilePoint], top: bool) -> Vec<u32> {
    let num_vertices = ring.len();
    if num_vertices < 3 {
        return Vec::new();
    }
    let mut indices = Vec::with_capacity(num_vertices.saturating_sub(2).saturating_mul(3));
    let mut pt1: usize = 0;
    let mut pt2: usize = num_vertices.saturating_sub(1);
    while pt2 > pt1.saturating_add(1) {
        let p1 = ring_pos(ring, pt1);
        let p2 = ring_pos(ring, pt2);
        let pa = ring_pos(ring, pt1.saturating_add(1));
        let pb = ring_pos(ring, pt2.saturating_sub(1));

        // Signed areas of the candidate triangles; a negative area means the
        // triangle is back-facing (or contains the opposite ring's next point).
        let tri_1a2 = signed_area(p1, pa, p2) >= 0.0 && signed_area(p2, pa, pb) >= 0.0;
        let tri_21b = signed_area(p2, p1, pb) >= 0.0 && signed_area(p1, pb, pa) >= 0.0;

        let use_tri1a2 = if !tri_1a2 {
            false
        } else if !tri_21b {
            true
        } else {
            // Both usable: keep the shorter diagonal (Firestorm's tie-break).
            let d1 = subtract2(p1, pa);
            let d2 = subtract2(p2, pb);
            dot2(d1, d1) < dot2(d2, d2)
        };

        let (a, b, c) = if use_tri1a2 {
            let tri = (pt1, pt1.saturating_add(1), pt2);
            pt1 = pt1.saturating_add(1);
            tri
        } else {
            let tri = (pt1, pt2.saturating_sub(1), pt2);
            pt2 = pt2.saturating_sub(1);
            tri
        };
        // The underside reverses the winding so its single flat normal points
        // down (Firestorm's "flipped backfacing from top").
        if top {
            indices.extend_from_slice(&[u32_from_usize(a), u32_from_usize(b), u32_from_usize(c)]);
        } else {
            indices.extend_from_slice(&[u32_from_usize(a), u32_from_usize(c), u32_from_usize(b)]);
        }
    }
    indices
}

/// The 2D profile position of ring point `index` (the origin if out of range).
fn ring_pos(ring: &[ProfilePoint], index: usize) -> [f32; 2] {
    ring.get(index).map_or([0.0, 0.0], |point| point.position)
}

/// Twice the signed area of the 2D triangle `a → b → c` (positive when wound
/// counter-clockwise); the reference viewer's cap back-face test.
fn signed_area(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (a[0] * b[1] - b[0] * a[1]) + (b[0] * c[1] - c[0] * b[1]) + (c[0] * a[1] - a[0] * c[1])
}

/// The 2D vector difference `a - b`.
fn subtract2(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// The 2D dot product `a · b`.
fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// The planar texture coordinate for a cap vertex at profile position `(x, y)`
/// (Firestorm `createCap`): the profile centred in the UV square, with the
/// underside mirrored in V so the cap reads the same way from both sides.
fn cap_uv(position: [f32; 2], top: bool) -> [f32; 2] {
    let [x, y] = position;
    if top {
        [x + CAP_UV_CENTRE, y + CAP_UV_CENTRE]
    } else {
        [x + CAP_UV_CENTRE, CAP_UV_CENTRE - y]
    }
}

/// The triangle-fan indices for a cap of `ring_count` ring points around
/// `centre_index` (Firestorm `createCap`'s fan branch): the underside is wound
/// the opposite way so its flat normal faces out.
fn cap_indices(ring_count: usize, centre_index: u32, top: bool) -> Vec<u32> {
    let triangles = ring_count.saturating_sub(1);
    let mut indices = Vec::with_capacity(triangles.saturating_mul(3));
    for i in 0..triangles {
        let a = u32_from_usize(i);
        let b = u32_from_usize(i.saturating_add(1));
        if top {
            indices.extend_from_slice(&[centre_index, a, b]);
        } else {
            indices.extend_from_slice(&[centre_index, b, a]);
        }
    }
    indices
}

/// The single flat normal of a cap, from its first fan triangle (Firestorm
/// `createCap`'s cross product), falling back to the ±Z axis when the triangle
/// is degenerate.
fn cap_normal(positions: &[[f32; 3]], indices: &[u32], top: bool) -> [f32; 3] {
    let triangle = first_triangle(positions, indices);
    if let Some((p0, p1, p2)) = triangle {
        let normal = cross(subtract(p1, p0), subtract(p2, p0));
        if dot(normal, normal) > NORMAL_EPSILON {
            return normalize(normal);
        }
    }
    if top {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, 0.0, -1.0]
    }
}

/// The three positions of the first triangle in `indices`, or `None` when the
/// index list is empty or references a missing vertex.
fn first_triangle(
    positions: &[[f32; 3]],
    indices: &[u32],
) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
    let (i0, i1, i2) = match indices {
        [i0, i1, i2, ..] => (*i0, *i1, *i2),
        _short => return None,
    };
    let p0 = positions.get(usize_from_u32(i0)).copied()?;
    let p1 = positions.get(usize_from_u32(i1)).copied()?;
    let p2 = positions.get(usize_from_u32(i2)).copied()?;
    Some((p0, p1, p2))
}

/// Accumulate an (area-weighted) normal at each vertex by summing every incident
/// triangle's un-normalized face normal (Firestorm `createSide`'s normal
/// accumulation); the result is normalized by the caller.
fn accumulate_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0_f32; 3]; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let (i0, i1, i2) = match triangle {
            [i0, i1, i2] => (
                usize_from_u32(*i0),
                usize_from_u32(*i1),
                usize_from_u32(*i2),
            ),
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
    normals
}

/// Replace two vertices' normals with their sum, sharing a seam or loop
/// (Firestorm `createSide`'s `n.setAdd(...)` wrap).
fn share_normals(normals: &mut [[f32; 3]], a: usize, b: usize) {
    let (Some(na), Some(nb)) = (normals.get(a).copied(), normals.get(b).copied()) else {
        return;
    };
    let sum = [na[0] + nb[0], na[1] + nb[1], na[2] + nb[2]];
    set_normal(normals, a, sum);
    set_normal(normals, b, sum);
}

/// Add `value` into the accumulated normal at `index` (a no-op if out of range).
fn add_normal(normals: &mut [[f32; 3]], index: usize, value: [f32; 3]) {
    if let Some(slot) = normals.get_mut(index) {
        slot[0] += value[0];
        slot[1] += value[1];
        slot[2] += value[2];
    }
}

/// Overwrite the normal at `index` (a no-op if out of range).
fn set_normal(normals: &mut [[f32; 3]], index: usize, value: [f32; 3]) {
    if let Some(slot) = normals.get_mut(index) {
        *slot = value;
    }
}

/// Normalize every normal in place, leaving a degenerate (near-zero) normal as
/// an up-vector so no face carries an invalid direction.
fn normalize_all(normals: &mut [[f32; 3]]) {
    for normal in normals.iter_mut() {
        if dot(*normal, *normal) > NORMAL_EPSILON {
            *normal = normalize(*normal);
        } else {
            *normal = [0.0, 0.0, 1.0];
        }
    }
}

/// The bounding-box centre of a set of 3D points (`(min + max) / 2`); an empty
/// set yields the origin.
fn bounds_centre(points: &[[f32; 3]]) -> [f32; 3] {
    let Some((first, rest)) = points.split_first() else {
        return [0.0; 3];
    };
    let mut min = *first;
    let mut max = *first;
    for point in rest {
        for axis in 0..3 {
            let value = point.get(axis).copied().unwrap_or(0.0);
            if let Some(lo) = min.get_mut(axis) {
                *lo = lo.min(value);
            }
            if let Some(hi) = max.get_mut(axis) {
                *hi = hi.max(value);
            }
        }
    }
    [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ]
}

/// The bounding-box centre of a set of 2D texture coordinates
/// (`(min + max) / 2`); an empty set yields the origin.
fn bounds_centre_2d(points: &[[f32; 2]]) -> [f32; 2] {
    let Some((first, rest)) = points.split_first() else {
        return [0.0; 2];
    };
    let mut min = *first;
    let mut max = *first;
    for point in rest {
        for axis in 0..2 {
            let value = point.get(axis).copied().unwrap_or(0.0);
            if let Some(lo) = min.get_mut(axis) {
                *lo = lo.min(value);
            }
            if let Some(hi) = max.get_mut(axis) {
                *hi = hi.max(value);
            }
        }
    }
    [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5]
}

/// The flat grid index of profile column `s`, path row `t`, for a `num_s`-wide
/// strip (`s + num_s * t`).
const fn grid_index(s: usize, t: usize, num_s: usize) -> usize {
    t.saturating_mul(num_s).saturating_add(s)
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

/// The squared Euclidean distance between two points.
fn squared_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = subtract(a, b);
    dot(d, d)
}

/// Widen a `u32` to `usize` (lossless on every supported target).
fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// Narrow a `usize` vertex index to `u32` for the index buffer; prim vertex
/// counts are far below `u32::MAX`, so a saturating conversion never loses a
/// real index.
fn u32_from_usize(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Narrow a `usize` face index to the `u16` a [`PrimFaceId`] wraps; a prim has
/// far fewer than `u16::MAX` faces, so a saturating conversion never loses one.
fn u16_from_usize(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// Convert a small grid index to `f32` for a planar interpolation coefficient;
/// box-cap grid counts are tiny, so the conversion is exact.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "value is a tiny grid index that converts to f32 exactly"
)]
const fn f32_from_usize(value: usize) -> f32 {
    value as f32
}

/// Floor a small, non-negative detail product to `u32`; a negative or
/// non-finite value (which the parameters cannot produce) maps to `0`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is a small non-negative detail product; its floor fits a u32 exactly"
)]
fn floor_to_u32(value: f32) -> u32 {
    if value.is_finite() && value >= 0.0 {
        value.floor() as u32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::tessellate;
    use crate::PrimLod;
    use crate::geometry::{PrimFace, PrimMesh};
    use crate::shape::PrimShape;
    use pretty_assertions::assert_eq;
    use sl_proto::PrimShapeParams;

    /// The wire params for the viewer's default new prim (a unit box): square
    /// profile, line path, full top size, no cut / hollow / twist.
    fn default_box_params() -> PrimShapeParams {
        PrimShapeParams {
            path_curve: 0x10,
            profile_curve: 0x01,
            path_begin: 0,
            path_end: 0,
            path_scale_x: 100,
            path_scale_y: 100,
            path_shear_x: 0,
            path_shear_y: 0,
            path_twist: 0,
            path_twist_begin: 0,
            path_radius_offset: 0,
            path_taper_x: 0,
            path_taper_y: 0,
            path_revolutions: 0,
            path_skew: 0,
            profile_begin: 0,
            profile_end: 0,
            profile_hollow: 0,
        }
    }

    /// Assert a single face is internally consistent: the four vertex arrays are
    /// parallel, the index list is a non-empty multiple of three, every index is
    /// in range, and every normal is unit length.
    fn assert_face_integrity(face: &PrimFace) {
        let count = face.positions.len();
        assert!(count >= 3, "face {:?} has too few vertices", face.face_id);
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
    }

    /// Every non-empty face of a mesh is internally consistent.
    fn assert_mesh_integrity(mesh: &PrimMesh) {
        for face in &mesh.faces {
            if !face.is_empty() {
                assert_face_integrity(face);
            }
        }
    }

    #[test]
    fn box_has_six_consistent_faces() {
        let shape = PrimShape::from_params(&default_box_params());
        let mesh = tessellate(&shape, PrimLod::High);
        // Four sides + two caps.
        assert_eq!(mesh.face_count(), 6);
        assert_mesh_integrity(&mesh);
        assert!(mesh.triangle_count() >= 6);
    }

    #[test]
    fn cylinder_sweeps_a_round_side_and_two_caps() {
        let mut params = default_box_params();
        params.profile_curve = 0x00;
        let shape = PrimShape::from_params(&params);
        let mesh = tessellate(&shape, PrimLod::High);
        // One round outer face plus two path caps.
        assert_eq!(mesh.face_count(), 3);
        assert_mesh_integrity(&mesh);
        // The round side is finely tessellated at High detail.
        assert!(mesh.triangle_count() > 40);
    }

    #[test]
    fn sphere_sweeps_a_single_closed_face() {
        let mut params = default_box_params();
        params.profile_curve = 0x05;
        params.path_curve = 0x30;
        let shape = PrimShape::from_params(&params);
        let mesh = tessellate(&shape, PrimLod::High);
        // A solid, uncut sphere is a single face (no path caps: the path closes).
        assert_eq!(mesh.face_count(), 1);
        assert_mesh_integrity(&mesh);
        assert!(mesh.vertex_count() > 0);
    }

    #[test]
    fn torus_sweeps_a_closed_ring() {
        let mut params = default_box_params();
        params.profile_curve = 0x00;
        params.path_curve = 0x20;
        let shape = PrimShape::from_params(&params);
        let mesh = tessellate(&shape, PrimLod::High);
        // A default torus is one continuous outer face.
        assert_eq!(mesh.face_count(), 1);
        assert_mesh_integrity(&mesh);
        assert!(mesh.triangle_count() > 100);
    }

    #[test]
    fn hollow_box_adds_an_inner_face() {
        let solid = tessellate(
            &PrimShape::from_params(&default_box_params()),
            PrimLod::High,
        );
        let mut params = default_box_params();
        // Half hollow, square hole (same as the outer square profile).
        params.profile_hollow = 25000;
        let hollow = tessellate(&PrimShape::from_params(&params), PrimLod::High);
        // A solid box is four sides + two caps; a hollow box adds the inner-side
        // wall face — one more drawable face than the solid box.
        assert_eq!(solid.face_count(), 6);
        assert_eq!(hollow.face_count(), 7);
        assert_mesh_integrity(&hollow);
        // The inner wall doubles the swept ring, so more geometry overall.
        assert!(hollow.vertex_count() > solid.vertex_count());
    }

    #[test]
    fn cut_prim_adds_cut_edge_faces() {
        let uncut = tessellate(
            &PrimShape::from_params(&default_box_params()),
            PrimLod::High,
        );
        let mut params = default_box_params();
        // Cut the profile to a quarter: begin 0.25, end 0.5.
        params.profile_begin = 12500;
        params.profile_end = 25000;
        let cut = tessellate(&PrimShape::from_params(&params), PrimLod::High);
        // Cutting the ring opens it: the four closed sides collapse to the one
        // spanned side (the 0.25..0.5 cut covers exactly one square edge), and
        // the two cut edges (profile-begin / profile-end) plus the two path caps
        // make five faces.
        assert_eq!(uncut.face_count(), 6);
        assert_eq!(cut.face_count(), 5);
        assert_mesh_integrity(&cut);
    }

    /// A hollow prim's path caps must be an annulus whose every triangle winds
    /// the same way (`+Z` for the top, `-Z` for the bottom) — the R7 fix. A
    /// centre fan over the `outer ++ reversed-inner` ring wound its inner half
    /// backwards, so half the cap was back-face culled (a see-through cap).
    #[test]
    fn hollow_cut_cylinder_caps_wind_consistently() {
        let mut params = default_box_params();
        params.profile_curve = 0x00; // circle
        params.path_curve = 0x10; // line
        params.profile_hollow = 47500; // 0.95
        params.profile_begin = 2000; // cut 0.04
        params.profile_end = 25500; // .. 0.51
        let shape = PrimShape::from_params(&params);
        let mesh = tessellate(&shape, PrimLod::High);
        assert_mesh_integrity(&mesh);

        // Faces 0 / 3 are the path-begin (top) / path-end (bottom) caps.
        let mut checked = 0_usize;
        for face in &mesh.faces {
            let want_top = match face.face_id.get() {
                0 => true,
                3 => false,
                _other => continue,
            };
            checked = checked.saturating_add(1);
            assert!(!face.is_empty(), "hollow cap carries geometry");
            // A hollow cap is an annulus: no centre vertex, one triangle per
            // ring point beyond the first two.
            assert_eq!(
                face.triangle_count(),
                face.vertex_count().saturating_sub(2),
                "hollow cap is an annulus (no centre fan)"
            );
            for tri in face.indices.chunks_exact(3) {
                let vertex = |slot: usize| -> [f32; 3] {
                    tri.get(slot)
                        .and_then(|&i| face.positions.get(usize::try_from(i).unwrap_or(0)))
                        .copied()
                        .unwrap_or([0.0; 3])
                };
                let (p0, p1, p2) = (vertex(0), vertex(1), vertex(2));
                let normal = super::cross(super::subtract(p1, p0), super::subtract(p2, p0));
                let z = normal.get(2).copied().unwrap_or(0.0);
                if want_top {
                    assert!(z >= 0.0, "top-cap triangle winds outward (+Z), got {z}");
                } else {
                    assert!(z <= 0.0, "bottom-cap triangle winds outward (-Z), got {z}");
                }
            }
        }
        assert_eq!(checked, 2, "both path caps were checked");
    }

    /// A plain box's path caps must be a two-triangle quad (four corner
    /// vertices, no centre vertex), not a centre fan — the reference viewer's
    /// `createUnCutCubeCap`. A centre fan drew an X / cross through any texture.
    #[test]
    fn box_caps_are_two_triangle_quads() {
        // At Lowest detail the box takes no per-edge split, so each cap is the
        // minimal four-corner quad — the clearest form of the fix.
        let shape = PrimShape::from_params(&default_box_params());
        let mesh = tessellate(&shape, PrimLod::Lowest);
        assert_mesh_integrity(&mesh);

        let mut checked = 0_usize;
        for face in &mesh.faces {
            let want_top = match face.face_id.get() {
                0 => true,
                3 => false,
                _other => continue,
            };
            checked = checked.saturating_add(1);
            // A quad: four corners, two triangles, no centre-fan vertex.
            assert_eq!(face.vertex_count(), 4, "box cap is four corners");
            assert_eq!(face.triangle_count(), 2, "box cap is two triangles");
            // Every corner UV sits on the unit square's corners (planar box UVs).
            for uv in &face.uvs {
                assert!(
                    (uv[0] <= 0.01 || uv[0] >= 0.99) && (uv[1] <= 0.01 || uv[1] >= 0.99),
                    "box-cap UV {uv:?} is a unit-square corner"
                );
            }
            assert_cap_winds(face, want_top);
        }
        assert_eq!(checked, 2, "both box caps were checked");
    }

    /// Even split (High detail) box caps stay an all-quad grid — never a centre
    /// fan — so every triangle still winds one way and no cross appears.
    #[test]
    fn split_box_caps_are_a_consistent_grid() {
        let shape = PrimShape::from_params(&default_box_params());
        let mesh = tessellate(&shape, PrimLod::High);
        for face in &mesh.faces {
            let want_top = match face.face_id.get() {
                0 => true,
                3 => false,
                _other => continue,
            };
            // A `(n+1)²`-vertex grid with `2·n²` triangles: a perfect square of
            // vertices and twice a square of triangles, never the fan's
            // `ring + 1` vertices / `ring` triangles.
            let verts = face.vertex_count();
            let side = (1..=verts)
                .find(|n| n.saturating_mul(*n) == verts)
                .unwrap_or(0);
            assert_eq!(side.saturating_mul(side), verts, "cap is a square grid");
            let cells = side.saturating_sub(1);
            assert_eq!(
                face.triangle_count(),
                cells.saturating_mul(cells).saturating_mul(2),
                "cap is two triangles per grid cell"
            );
            assert_cap_winds(face, want_top);
        }
    }

    /// Assert every triangle of a path cap winds outward (`+Z` top / `-Z`
    /// bottom).
    fn assert_cap_winds(face: &PrimFace, want_top: bool) {
        for tri in face.indices.chunks_exact(3) {
            let vertex = |slot: usize| -> [f32; 3] {
                tri.get(slot)
                    .and_then(|&i| face.positions.get(usize::try_from(i).unwrap_or(0)))
                    .copied()
                    .unwrap_or([0.0; 3])
            };
            let (p0, p1, p2) = (vertex(0), vertex(1), vertex(2));
            let z = super::cross(super::subtract(p1, p0), super::subtract(p2, p0))
                .get(2)
                .copied()
                .unwrap_or(0.0);
            if want_top {
                assert!(z >= 0.0, "top cap triangle winds +Z, got {z}");
            } else {
                assert!(z <= 0.0, "bottom cap triangle winds -Z, got {z}");
            }
        }
    }

    #[test]
    fn detail_scales_geometry() {
        let shape = PrimShape::from_params(&default_box_params());
        let low = tessellate(&shape, PrimLod::Lowest);
        let high = tessellate(&shape, PrimLod::High);
        // The box faces are the same, but High applies the per-edge split.
        assert_eq!(low.face_count(), high.face_count());
        assert!(high.vertex_count() > low.vertex_count());
    }

    #[test]
    fn every_face_positions_are_finite() {
        // A twisted, tapered, sheared torus exercises the full sweep transform.
        let mut params = default_box_params();
        params.profile_curve = 0x00;
        params.path_curve = 0x20;
        params.path_twist = 60;
        params.path_taper_x = -40;
        params.path_shear_x = 10;
        let shape = PrimShape::from_params(&params);
        let mesh = tessellate(&shape, PrimLod::Medium);
        assert_mesh_integrity(&mesh);
        for face in &mesh.faces {
            for position in &face.positions {
                for value in position {
                    assert!(value.is_finite(), "swept position {position:?} is finite");
                }
            }
        }
    }
}
