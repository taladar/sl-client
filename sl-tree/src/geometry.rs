//! Procedural `LLVOTree` branch / leaf geometry generation, ported from
//! Firestorm's `LLVOTree::updateGeometry` / `genBranchPipeline`.
//!
//! A tree's visible form is generated on the CPU from its [`TreeSpecies`]
//! parameters (there is no fetched geometry asset): a recursive *branch
//! pipeline* stamps transformed copies of two reference primitives — a tapered
//! trunk/branch **cylinder** and a crossed-quad **leaf** card — into one mesh.
//! The recursion emits a cylinder for each branch segment down to the leaf
//! level, then a leaf card at the tips, recursing both to spawn child branches
//! and to continue the trunk.
//!
//! The output is deliberately **Bevy-free**: a [`TreeMesh`] of plain position /
//! normal / uv / index buffers in Second Life's right-handed **Z-up** space,
//! generated at **unit outer scale** — the caller applies the tree's world
//! placement (the reference viewer's `radius = scale.length() * 0.05` uniform
//! scale, the fixed 90° yaw, and the object's position / rotation) at the
//! transform boundary, exactly as the prim / mesh paths do.
//!
//! The trunk radius carries the reference's **Perlin turbulence** bark
//! irregularity (the crate-internal `noise` module, a port of
//! `LLPerlinNoise::turbulence3` seeded to match the reference's default `rand()`
//! stream). One faithful simplification remains: **wind / trunk-bend** is not
//! simulated, so the
//! effective branch droop is the reference's rest value `species.droop + 25°`
//! (`mDroop + 25 * (1 - trunkBend)`, with `trunkBend == 0`).

use crate::noise::turbulence3;
use crate::species::TreeSpecies;

/// Number of trunk levels of detail the reference viewer defines
/// (`sMAX_NUM_TREE_LOD_LEVELS`).
pub const TREE_LOD_LEVELS: usize = 4;

/// Cylinder slice count per level of detail (`LLVOTree::sLODSlices`). More slices
/// means a rounder trunk and more geometry; index 0 is the finest.
const LOD_SLICES: [usize; TREE_LOD_LEVELS] = [10, 5, 4, 3];

/// The uniform scale factor the reference viewer applies to the whole generated
/// tree (`radius = getScale().magVec() * 0.05`). Exposed so the renderer can size
/// the unit-outer-scale [`TreeMesh`] this module produces.
pub const RADIUS_SCALE_FACTOR: f32 = 0.05;

/// The fixed yaw (degrees, about the Second Life Z axis) the reference viewer
/// applies to every tree before the object's own rotation
/// (`LLQuaternion(90°, (0,0,1))`). Exposed so the renderer can reproduce it at
/// the transform boundary.
pub const YAW_DEGREES: f32 = 90.0;

/// The extra droop (degrees) the reference adds to the species value at rest
/// (`mDroop + 25 * (1 - trunkBend)`, with no wind so `trunkBend == 0`).
const REST_DROOP_BONUS: f32 = 25.0;

/// Leaf card left texture coordinate (`LLVOTree::LEAF_LEFT`).
const LEAF_LEFT: f32 = 0.52;
/// Leaf card right texture coordinate (`LLVOTree::LEAF_RIGHT`).
const LEAF_RIGHT: f32 = 0.98;
/// Leaf card top texture coordinate (`LLVOTree::LEAF_TOP`).
const LEAF_TOP: f32 = 1.0;
/// Leaf card bottom texture coordinate (`LLVOTree::LEAF_BOTTOM`).
const LEAF_BOTTOM: f32 = 0.52;
/// Leaf card width (`LLVOTree::LEAF_WIDTH`).
const LEAF_WIDTH: f32 = 1.0;

/// `sqrt(1/2)`, the reference viewer's `SRR2` leaf-normal component.
const SRR2: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// `sqrt(1/3)`, the reference viewer's `SRR3` leaf-normal component.
const SRR3: f32 = 0.577_350_26;

/// Degrees → radians.
const DEG_TO_RAD: f32 = std::f32::consts::PI / 180.0;

/// Texture-coordinate inset (in `u`) that keeps the trunk seam column off the
/// atlas's left edge (`u = 0`).
///
/// The species texture is an atlas — trunk bark in the left half (`u ∈ [0, 0.5]`)
/// and leaf cards in the right (`u ∈ [0.52, 0.98]`) — so its outer edges are
/// transparent. A renderer sampling the seam column at exactly `u = 0` with
/// repeat addressing and bilinear filtering blends in the transparent far edge
/// (`u ≈ 1`), which an alpha-mask then clips into a thin see-through slit down one
/// side of the trunk. Insetting the seam column a hair keeps the sample on opaque
/// bark. The reference viewer already insets the *closing* column to `0.490` (its
/// "slight slop factor to avoid edges on leaves"); this mirrors that on the
/// opening column. A couple of percent of the bark half is imperceptible.
const TRUNK_U_MARGIN: f32 = 0.01;

/// Base radius of the reference trunk cylinder (`base_radius` in `updateGeometry`).
const CYLINDER_BASE_RADIUS: f32 = 0.65;
/// Height the trunk cylinder caps are nudged past the `[0, 1]` column so the
/// top / bottom pinch to a peak (`cap_nudge`).
const CYLINDER_CAP_NUDGE: f32 = 0.1;

/// A 3×3 matrix, row-major (row `i` is where basis vector `e_i` maps under the
/// row-vector convention `v' = v·M`).
type Mat3 = [[f32; 3]; 3];

/// One of the four branching trunk levels of detail (`LLVOTree::mTrunkLOD`),
/// finest first. Selects the trunk cylinder slice count; a coarser level renders
/// fewer, blockier branches. A [`crate::billboard_geometry`] imposter is the
/// still-coarser far fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeLod {
    /// Finest branching geometry (10 slices per trunk).
    Highest,
    /// 5 slices per trunk.
    High,
    /// 4 slices per trunk.
    Medium,
    /// Coarsest branching geometry (3 slices per trunk).
    Low,
}

impl TreeLod {
    /// The finest level.
    pub const FINEST: Self = Self::Highest;
    /// The coarsest branching level (still geometry, not the billboard imposter).
    pub const COARSEST: Self = Self::Low;

    /// The trunk cylinder slice count for this level (`LLVOTree::sLODSlices`).
    #[must_use]
    pub fn slices(self) -> usize {
        LOD_SLICES.get(self.index()).copied().unwrap_or(3)
    }

    /// This level's index into the reference LOD tables (`0` = finest).
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Highest => 0,
            Self::High => 1,
            Self::Medium => 2,
            Self::Low => 3,
        }
    }

    /// The level for a reference LOD index (`0` = finest), clamped into range.
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Highest,
            1 => Self::High,
            2 => Self::Medium,
            _ => Self::Low,
        }
    }
}

/// A generated tree mesh: a single `TriangleList` in Second Life's right-handed
/// Z-up space at unit outer scale, ready to be bridged to a renderer's mesh type.
///
/// All four buffers are parallel where present: `positions`, `normals` and `uvs`
/// share one length, and `indices` reference them (three per triangle).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TreeMesh {
    /// Vertex positions (metres, unit outer scale).
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex normals (unit length).
    pub normals: Vec<[f32; 3]>,
    /// Per-vertex texture coordinates into the species texture.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle indices into the vertex buffers.
    pub indices: Vec<u32>,
}

impl TreeMesh {
    /// The current vertex count as the `u32` index base for appending a
    /// transformed template copy (tree vertex counts never approach `u32::MAX`).
    fn index_base(&self) -> u32 {
        u32_from_usize(self.positions.len())
    }
}

/// A minimal affine transform (a 3×3 linear part plus a translation) in the
/// reference viewer's **row-vector** convention: a point maps as `p' = p·L + t`,
/// and composing "`a` then `b`" is [`Affine::then`] (the reference's `a *= b`).
///
/// This mirrors `LLMatrix4` closely enough to port `genBranchPipeline` verbatim
/// without pulling in a matrix dependency, matching the sibling geometry crates'
/// hand-rolled `[f32; 3]` math.
#[derive(Debug, Clone, Copy)]
struct Affine {
    /// Linear part, row-major: row `i` is where basis vector `e_i` maps.
    l: Mat3,
    /// Translation, added after the linear part.
    t: [f32; 3],
}

impl Affine {
    /// The identity transform.
    const IDENTITY: Self = Self {
        l: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        t: [0.0, 0.0, 0.0],
    };

    /// A non-uniform scale about the origin.
    const fn scale(sx: f32, sy: f32, sz: f32) -> Self {
        Self {
            l: [[sx, 0.0, 0.0], [0.0, sy, 0.0], [0.0, 0.0, sz]],
            t: [0.0, 0.0, 0.0],
        }
    }

    /// A pure translation.
    const fn translation(x: f32, y: f32, z: f32) -> Self {
        Self {
            l: Self::IDENTITY.l,
            t: [x, y, z],
        }
    }

    /// The rotation of a [`Quat`] as an affine (its `LLMatrix3` equivalent).
    fn rotation(q: Quat) -> Self {
        Self {
            l: q.to_matrix3(),
            t: [0.0, 0.0, 0.0],
        }
    }

    /// Compose so that the result applies `self` first, then `other` — the
    /// reference viewer's `self *= other` (a point maps `p·self·other`).
    fn then(self, other: Self) -> Self {
        Self {
            l: mat_mul(self.l, other.l),
            // Translation: self.t mapped through other's linear part, plus other.t.
            t: add(mul_row(self.t, other.l), other.t),
        }
    }

    /// Transform a position (`p·L + t`).
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        add(mul_row(p, self.l), self.t)
    }

    /// The normal matrix for this transform: `transpose(inverse(L))`, so a normal
    /// stays perpendicular under a non-uniform scale (the reference's `norm_mat`).
    /// Falls back to the linear part itself if it is singular (never in practice).
    fn normal_matrix(&self) -> Mat3 {
        inverse_transpose(self.l).unwrap_or(self.l)
    }
}

/// A quaternion in the reference viewer's `LLQuaternion` convention (`x, y, z`
/// vector part, `w` scalar), enough to port the branch-pipeline rotations.
#[derive(Debug, Clone, Copy)]
struct Quat {
    /// X component of the vector part.
    x: f32,
    /// Y component of the vector part.
    y: f32,
    /// Z component of the vector part.
    z: f32,
    /// Scalar part.
    w: f32,
}

impl Quat {
    /// A rotation of `angle` radians about `axis` (`LLQuaternion(angle, axis)`).
    fn from_angle_axis(angle: f32, axis: [f32; 3]) -> Self {
        let [ax, ay, az] = normalize(axis);
        let half = angle * 0.5;
        let s = half.sin();
        Self {
            x: ax * s,
            y: ay * s,
            z: az * s,
            w: half.cos(),
        }
    }

    /// The `LLQuaternion` product `self * other` — verbatim from
    /// `operator*(const LLQuaternion &a, const LLQuaternion &b)`, so that a
    /// row-vector maps `v·(a*b) == (v·a)·b`.
    fn mul(self, other: Self) -> Self {
        let a = self;
        let b = other;
        Self {
            x: b.w * a.x + b.x * a.w + b.y * a.z - b.z * a.y,
            y: b.w * a.y + b.y * a.w + b.z * a.x - b.x * a.z,
            z: b.w * a.z + b.z * a.w + b.x * a.y - b.y * a.x,
            w: b.w * a.w - b.x * a.x - b.y * a.y - b.z * a.z,
        }
    }

    /// The row-major `LLMatrix3` equivalent (`LLQuaternion::getMatrix3`), so that
    /// `v·M` rotates `v` by this quaternion.
    fn to_matrix3(self) -> Mat3 {
        let Self { x, y, z, w } = self;
        let (xx, xy, xz, xw) = (x * x, x * y, x * z, x * w);
        let (yy, yz, yw) = (y * y, y * z, y * w);
        let (zz, zw) = (z * z, z * w);
        [
            [1.0 - 2.0 * (yy + zz), 2.0 * (xy + zw), 2.0 * (xz - yw)],
            [2.0 * (xy - zw), 1.0 - 2.0 * (xx + zz), 2.0 * (yz + xw)],
            [2.0 * (xz + yw), 2.0 * (yz - xw), 1.0 - 2.0 * (xx + yy)],
        ]
    }
}

/// Row-vector × 3×3 matrix (`v'[j] = sum_i v[i] * m[i][j]`).
fn mul_row(v: [f32; 3], m: Mat3) -> [f32; 3] {
    let [x, y, z] = v;
    let [[m00, m01, m02], [m10, m11, m12], [m20, m21, m22]] = m;
    [
        x * m00 + y * m10 + z * m20,
        x * m01 + y * m11 + z * m21,
        x * m02 + y * m12 + z * m22,
    ]
}

/// Row-vector-convention 3×3 matrix product (`(a·b)[i][j] = sum_k a[i][k]*b[k][j]`).
fn mat_mul(a: Mat3, b: Mat3) -> Mat3 {
    let [ra, rb, rc] = a;
    [mul_row(ra, b), mul_row(rb, b), mul_row(rc, b)]
}

/// Vector sum.
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax + bx, ay + by, az + bz]
}

/// Normalize a vector, returning the input unchanged if it is (near) zero length.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = v;
    let len = (x * x + y * y + z * z).sqrt();
    if len <= f32::EPSILON {
        v
    } else {
        [x / len, y / len, z / len]
    }
}

/// `transpose(inverse(m))` for a 3×3 matrix, or `None` if it is singular.
fn inverse_transpose(m: Mat3) -> Option<Mat3> {
    let [[m00, m01, m02], [m10, m11, m12], [m20, m21, m22]] = m;
    // Cofactor matrix. `inverse = adjugate / det` and `adjugate = transpose(cofactor)`,
    // so `transpose(inverse) = cofactor / det`.
    let c00 = m11 * m22 - m12 * m21;
    let c01 = m12 * m20 - m10 * m22;
    let c02 = m10 * m21 - m11 * m20;
    let c10 = m02 * m21 - m01 * m22;
    let c11 = m00 * m22 - m02 * m20;
    let c12 = m01 * m20 - m00 * m21;
    let c20 = m01 * m12 - m02 * m11;
    let c21 = m02 * m10 - m00 * m12;
    let c22 = m00 * m11 - m01 * m10;
    let det = m00 * c00 + m01 * c01 + m02 * c02;
    if det.abs() <= f32::EPSILON {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        [c00 * inv_det, c01 * inv_det, c02 * inv_det],
        [c10 * inv_det, c11 * inv_det, c12 * inv_det],
        [c20 * inv_det, c21 * inv_det, c22 * inv_det],
    ])
}

/// A reference primitive (the shared leaf or a trunk cylinder) that the branch
/// pipeline stamps transformed copies of into the output mesh — the reference
/// viewer's `mReferenceBuffer` sub-ranges.
#[derive(Debug, Clone)]
struct Template {
    /// Template vertex positions.
    positions: Vec<[f32; 3]>,
    /// Template per-vertex normals.
    normals: Vec<[f32; 3]>,
    /// Template per-vertex texture coordinates.
    uvs: Vec<[f32; 2]>,
    /// Template triangle indices (into the template's own vertices).
    indices: Vec<u32>,
}

impl Template {
    /// Append a copy of this template transformed by `matrix` to `out` (the
    /// reference viewer's `appendMesh`): positions by the affine, normals by its
    /// normal matrix (re-normalized), uvs copied, indices rebased.
    fn append_to(&self, out: &mut TreeMesh, matrix: &Affine) {
        let base = out.index_base();
        let norm_mat = matrix.normal_matrix();
        for ((&p, &n), &uv) in self.positions.iter().zip(&self.normals).zip(&self.uvs) {
            out.positions.push(matrix.point(p));
            out.normals.push(normalize(mul_row(n, norm_mat)));
            out.uvs.push(uv);
        }
        for &idx in &self.indices {
            out.indices.push(idx.saturating_add(base));
        }
    }
}

/// The crossed-quad leaf card template (`LEAF_VERTICES` / `LEAF_INDICES`), shared
/// by every species — two 90°-crossed quads, each doubled with inverse winding so
/// the leaves are visible from both sides.
fn leaf_template() -> Template {
    let half = 0.5 * LEAF_WIDTH;
    // Vertices ported verbatim from `LLVOTree::updateGeometry`'s leaf section.
    let positions = vec![
        [-half, 0.0, 0.0],
        [half, 0.0, 1.0],
        [-half, 0.0, 1.0],
        [half, 0.0, 0.0],
        [-half, 0.0, 0.0],
        [half, 0.0, 1.0],
        [-half, 0.0, 1.0],
        [half, 0.0, 0.0],
        [0.0, -half, 0.0],
        [0.0, half, 1.0],
        [0.0, -half, 1.0],
        [0.0, half, 0.0],
        [0.0, -half, 0.0],
        [0.0, half, 1.0],
        [0.0, -half, 1.0],
        [0.0, half, 0.0],
    ];
    let normals = vec![
        [-SRR2, -SRR2, 0.0],
        [SRR3, -SRR3, SRR3],
        [-SRR3, -SRR3, SRR3],
        [SRR2, -SRR2, 0.0],
        [-SRR2, SRR2, 0.0],
        [SRR3, SRR3, SRR3],
        [-SRR3, SRR3, SRR3],
        [SRR2, SRR2, 0.0],
        [SRR2, -SRR2, 0.0],
        [SRR3, SRR3, SRR3],
        [SRR3, -SRR3, SRR3],
        [SRR2, SRR2, 0.0],
        [-SRR2, -SRR2, 0.0],
        [-SRR3, SRR3, SRR3],
        [-SRR3, -SRR3, SRR3],
        [-SRR2, SRR2, 0.0],
    ];
    let uvs = vec![
        [LEAF_LEFT, LEAF_BOTTOM],
        [LEAF_RIGHT, LEAF_TOP],
        [LEAF_LEFT, LEAF_TOP],
        [LEAF_RIGHT, LEAF_BOTTOM],
        [LEAF_LEFT, LEAF_BOTTOM],
        [LEAF_RIGHT, LEAF_TOP],
        [LEAF_LEFT, LEAF_TOP],
        [LEAF_RIGHT, LEAF_BOTTOM],
        [LEAF_LEFT, LEAF_BOTTOM],
        [LEAF_RIGHT, LEAF_TOP],
        [LEAF_LEFT, LEAF_TOP],
        [LEAF_RIGHT, LEAF_BOTTOM],
        [LEAF_LEFT, LEAF_BOTTOM],
        [LEAF_RIGHT, LEAF_TOP],
        [LEAF_LEFT, LEAF_TOP],
        [LEAF_RIGHT, LEAF_BOTTOM],
    ];
    // Index winding ported verbatim (four quads, two triangles each).
    let indices = vec![
        0, 1, 2, 0, 3, 1, // first leaf
        4, 6, 5, 4, 5, 7, // same leaf, inverse winding
        8, 9, 10, 8, 11, 9, // crossed leaf
        12, 14, 13, 12, 13, 15, // crossed leaf, inverse winding
    ];
    Template {
        positions,
        normals,
        uvs,
        indices,
    }
}

/// The tapered trunk / branch cylinder template for `slices` (the reference
/// viewer's per-LOD cylinder sub-range), a unit-height column along +Z pinched to
/// points at both caps. `taper` is the species top/base radius ratio,
/// `tex_z_repeat` its vertical trunk-texture repeat, and `noise_scale` /
/// `noise_mag` the species' Perlin bark spatial scale / amplitude
/// (`mNoiseScale` / `mNoiseMag`), which displace the radius per vertex.
fn cylinder_template(
    slices: usize,
    taper: f32,
    tex_z_repeat: f32,
    noise_scale: f32,
    noise_mag: f32,
) -> Template {
    let top_radius = CYLINDER_BASE_RADIUS * taper;
    let angle_inc = 360.0 / f32_from_usize(slices.saturating_sub(1));
    let z_inc = if slices > 3 {
        1.0 / f32_from_usize(slices.saturating_sub(3))
    } else {
        1.0
    };

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for i in 0..slices {
        let (z, r0) = if i == 0 {
            (-CYLINDER_CAP_NUDGE, 0.0)
        } else if i == slices.saturating_sub(1) {
            (1.0 + CYLINDER_CAP_NUDGE, 0.0)
        } else {
            let z = f32_from_usize(i.saturating_sub(1)) * z_inc;
            (
                z,
                CYLINDER_BASE_RADIUS + (top_radius - CYLINDER_BASE_RADIUS) * z,
            )
        };
        for j in 0..slices {
            let last = j == slices.saturating_sub(1);
            let angle = if last {
                0.0
            } else {
                f32_from_usize(j) * angle_inc
            };
            let x1 = (angle * DEG_TO_RAD).cos();
            let y1 = (angle * DEG_TO_RAD).sin();
            // Radius bulges toward the trunk's lower third (`height == 1`), then is
            // displaced by Perlin turbulence sampled in the reference's noise space
            // (x/y scaled by the radius, z by 4×), giving irregular bark.
            let start_radius = r0 * (1.0 + 1.2 * (z - 0.66).abs());
            let turbulence = turbulence3(
                x1 * start_radius * noise_scale,
                y1 * start_radius * noise_scale,
                z * 4.0 * noise_scale,
            );
            let radius = start_radius + turbulence * noise_mag;
            // Keep the seam (`u = 0`) column off the atlas edge so a repeat-wrapped
            // bilinear sample does not bleed the transparent far edge into the trunk.
            let u = if last {
                0.490
            } else {
                ((angle / 360.0) * 0.5).max(TRUNK_U_MARGIN)
            };
            let v = (1.0 - z / 2.0) * tex_z_repeat;
            positions.push([x1 * radius, y1 * radius, z]);
            normals.push([x1, y1, 0.0]);
            uvs.push([u, v]);
        }
    }

    let mut indices = Vec::new();
    let edges = slices.saturating_sub(1);
    for i in 0..edges {
        for j in 0..edges {
            let j1 = j.saturating_add(1);
            let x1_offset = if j1 == slices { 0 } else { j1 };
            let row = i.saturating_mul(slices);
            let next = i.saturating_add(1).saturating_mul(slices);
            for corner in [
                j.saturating_add(row),
                x1_offset.saturating_add(next),
                j.saturating_add(next),
                j.saturating_add(row),
                x1_offset.saturating_add(row),
                x1_offset.saturating_add(next),
            ] {
                indices.push(u32_from_usize(corner));
            }
        }
    }

    Template {
        positions,
        normals,
        uvs,
        indices,
    }
}

/// Recursively stamp the branch pipeline into `out` (the reference viewer's
/// `genBranchPipeline`), emitting a trunk/branch cylinder and recursing to child
/// branches and the continuing trunk until the leaf level, where a leaf card is
/// stamped instead.
#[expect(
    clippy::too_many_arguments,
    reason = "a verbatim port of LLVOTree::genBranchPipeline's parameter list"
)]
fn gen_branch(
    out: &mut TreeMesh,
    cylinder: &Template,
    leaf: &Template,
    species: &TreeSpecies,
    matrix: Affine,
    depth: u16,
    trunk_depth: u8,
    scale: f32,
    droop: f32,
) {
    // A trunk segment (rather than a side branch): the reference's
    // `trunk_depth || (scale == 1.f)`. `scale` stays exactly `1.0` only on the
    // trunk chain (side branches multiply by `scale_step < 1`, except kelp whose
    // `scale_step == 1` keeps its single stem a trunk — matching the reference).
    let is_trunk = trunk_depth != 0 || (scale - 1.0).abs() <= f32::EPSILON;
    let length = if is_trunk {
        species.trunk_length
    } else {
        species.branch_length
    };
    let aspect = if is_trunk {
        species.trunk_aspect
    } else {
        species.branch_aspect
    };
    let branches = species.branches;
    let constant_twist = 360.0 / branches;

    // `stop_level` is 0 for a rendered tree.
    if depth == 0 {
        // Leaf level: a crossed-quad card scaled by the leaf size.
        let leaf_size = scale * species.leaf_scale;
        let scale_mat = Affine::scale(leaf_size, leaf_size, leaf_size).then(matrix);
        leaf.append_to(out, &scale_mat);
        return;
    }

    // The trunk / branch cylinder for this segment.
    let width = scale * length * aspect;
    let scale_mat = Affine::scale(width, width, scale * length).then(matrix);
    cylinder.append_to(out, &scale_mat);

    // Child branches, fanned around the segment top.
    let branch_count = usize_from_f32_trunc(branches);
    for i in 0..branch_count {
        let trans_mat = Affine::translation(0.0, 0.0, scale * length).then(matrix);
        let twist = if i % 2 == 0 {
            species.twist
        } else {
            -species.twist
        };
        let z_angle = (constant_twist + twist) * f32_from_usize(i);
        let rot = Quat::from_angle_axis(20.0 * DEG_TO_RAD, [0.0, 0.0, 1.0])
            .mul(Quat::from_angle_axis(droop * DEG_TO_RAD, [0.0, 1.0, 0.0]))
            .mul(Quat::from_angle_axis(z_angle * DEG_TO_RAD, [0.0, 0.0, 1.0]));
        let rot_mat = Affine::rotation(rot).then(trans_mat);
        gen_branch(
            out,
            cylinder,
            leaf,
            species,
            rot_mat,
            depth.saturating_sub(1),
            0,
            scale * species.scale_step,
            droop,
        );
    }

    // Continue the trunk, rotated a little about Z as it ascends.
    if trunk_depth != 0 {
        let trans_mat = Affine::translation(0.0, 0.0, scale * length).then(matrix);
        let rot = Quat::from_angle_axis(70.5 * DEG_TO_RAD, [0.0, 0.0, 1.0]);
        let rot_mat = Affine::rotation(rot).then(trans_mat);
        gen_branch(
            out,
            cylinder,
            leaf,
            species,
            rot_mat,
            depth,
            trunk_depth.saturating_sub(1),
            scale * species.scale_step,
            droop,
        );
    }
}

/// Generate a tree's branch / leaf geometry for `species` at trunk level of
/// detail `lod`, in Second Life Z-up space at unit outer scale.
///
/// The mesh stands on the origin along +Z; the caller applies the reference
/// viewer's uniform [`RADIUS_SCALE_FACTOR`] scale (times the object's scale
/// length), the fixed [`YAW_DEGREES`] yaw, and the object's world placement at
/// the transform boundary. Textured with the species diffuse (its trunk region
/// for the cylinders, its leaf-card region for the leaves).
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `tree_geometry` reads clearly"
)]
pub fn tree_geometry(species: &TreeSpecies, lod: TreeLod) -> TreeMesh {
    let cylinder = cylinder_template(
        lod.slices(),
        species.taper,
        species.repeat_trunk_z,
        species.noise_scale,
        species.noise_mag,
    );
    let leaf = leaf_template();
    let mut out = TreeMesh::default();
    let droop = species.droop + REST_DROOP_BONUS;
    gen_branch(
        &mut out,
        &cylinder,
        &leaf,
        species,
        Affine::IDENTITY,
        u16::from(species.depth),
        species.trunk_depth,
        1.0,
        droop,
    );
    out
}

/// Generate a distant **billboard imposter** for `species`: two 90°-crossed
/// vertical quads (each double-sided) sampling the species' leaf-card texture
/// region, sized by the species billboard parameters, in Second Life Z-up space
/// at unit outer scale.
///
/// This is the far level of detail below [`TreeLod::COARSEST`] — a couple of
/// textured cards standing in for the whole tree once the branch geometry is too
/// small to be worth generating. It shares the tree's outer-scale transform, so
/// the caller sizes it the same way as [`tree_geometry`].
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `billboard_geometry` reads clearly"
)]
pub fn billboard_geometry(species: &TreeSpecies) -> TreeMesh {
    // Height along +Z and half-width across, in the same unit-outer-scale units as
    // the branch geometry; `billboard_ratio` is the height-to-width aspect.
    let height = species.billboard_scale;
    let half_width = 0.5 * height / species.billboard_ratio.max(f32::EPSILON);
    let mut out = TreeMesh::default();
    // Two crossed quads: one spanning X, one spanning Y.
    append_billboard_quad(&mut out, true, half_width, height);
    append_billboard_quad(&mut out, false, half_width, height);
    out
}

/// Append one double-sided vertical billboard quad to `out`, spanning the X axis
/// (`along_x`) or the Y axis, from `z = 0` to `z = height`.
fn append_billboard_quad(out: &mut TreeMesh, along_x: bool, half_width: f32, height: f32) {
    let base = out.index_base();
    // The four corners, and the outward normal (across the quad's face).
    let (corners, normal) = if along_x {
        (
            [
                [-half_width, 0.0, 0.0],
                [half_width, 0.0, 0.0],
                [half_width, 0.0, height],
                [-half_width, 0.0, height],
            ],
            [0.0, -1.0, 0.0],
        )
    } else {
        (
            [
                [0.0, -half_width, 0.0],
                [0.0, half_width, 0.0],
                [0.0, half_width, height],
                [0.0, -half_width, height],
            ],
            [-1.0, 0.0, 0.0],
        )
    };
    let uvs = [
        [LEAF_LEFT, LEAF_BOTTOM],
        [LEAF_RIGHT, LEAF_BOTTOM],
        [LEAF_RIGHT, LEAF_TOP],
        [LEAF_LEFT, LEAF_TOP],
    ];
    for (&corner, &uv) in corners.iter().zip(&uvs) {
        out.positions.push(corner);
        out.normals.push(normal);
        out.uvs.push(uv);
    }
    // Front face and back face (opposite winding) so it shows from both sides.
    for offset in [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] {
        out.indices.push(base.saturating_add(offset));
    }
}

/// Widen a small count to `f32`; tree slice / branch counts are tiny, well within
/// f32's exact-integer range.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "tree slice / branch / index counts are small, exact in f32"
)]
const fn f32_from_usize(value: usize) -> f32 {
    value as f32
}

/// Truncate a small, non-negative branch count to `usize`; a negative or
/// non-finite value (which the species table cannot produce) maps to `0`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is a small non-negative species branch count; its floor fits usize"
)]
fn usize_from_f32_trunc(value: f32) -> usize {
    if value.is_finite() && value >= 0.0 {
        value as usize
    } else {
        0
    }
}

/// Narrow a `usize` vertex index to `u32` for the index buffer; tree vertex
/// counts are far below `u32::MAX`, so a saturating conversion never loses one.
fn u32_from_usize(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{Affine, Quat, TreeLod, billboard_geometry, mul_row, tree_geometry};
    use crate::species::{TREE_SPECIES, tree_species};
    use pretty_assertions::assert_eq;

    /// Rotate a vector by a quaternion using the reference viewer's
    /// `operator*(const LLVector3&, const LLQuaternion&)` formula, to validate the
    /// [`Quat::to_matrix3`] conversion against it.
    fn ll_rotate(a: [f32; 3], rot: Quat) -> [f32; 3] {
        let [ax, ay, az] = a;
        let rw = -rot.x * ax - rot.y * ay - rot.z * az;
        let rx = rot.w * ax + rot.y * az - rot.z * ay;
        let ry = rot.w * ay + rot.z * ax - rot.x * az;
        let rz = rot.w * az + rot.x * ay - rot.y * ax;
        [
            -rw * rot.x + rx * rot.w - ry * rot.z + rz * rot.y,
            -rw * rot.y + ry * rot.w - rz * rot.x + rx * rot.z,
            -rw * rot.z + rz * rot.w - rx * rot.y + ry * rot.x,
        ]
    }

    fn approx(a: [f32; 3], b: [f32; 3]) {
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-5, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn quat_matrix_matches_reference_vector_rotation() {
        let rot = Quat::from_angle_axis(0.7, [0.0, 0.0, 1.0])
            .mul(Quat::from_angle_axis(0.4, [0.0, 1.0, 0.0]))
            .mul(Quat::from_angle_axis(1.1, [0.0, 0.0, 1.0]));
        let m = rot.to_matrix3();
        for v in [[1.0, 0.0, 0.0], [0.3, -0.7, 0.5], [0.0, 0.0, 1.0]] {
            approx(mul_row(v, m), ll_rotate(v, rot));
        }
    }

    #[test]
    fn compose_applies_self_then_other() {
        // Scale by 2 then translate by +1 in Z: the origin lands at (0,0,1); a
        // point at (0,0,1) lands at (0,0,3).
        let m = Affine::scale(2.0, 2.0, 2.0).then(Affine::translation(0.0, 0.0, 1.0));
        approx(m.point([0.0, 0.0, 0.0]), [0.0, 0.0, 1.0]);
        approx(m.point([0.0, 0.0, 1.0]), [0.0, 0.0, 3.0]);
    }

    #[test]
    fn every_species_generates_finite_indexed_geometry() {
        for species in &TREE_SPECIES {
            for lod in [
                TreeLod::Highest,
                TreeLod::High,
                TreeLod::Medium,
                TreeLod::Low,
            ] {
                let mesh = tree_geometry(species, lod);
                assert!(
                    !mesh.positions.is_empty(),
                    "{} {lod:?}: empty",
                    species.name
                );
                assert_eq!(mesh.positions.len(), mesh.normals.len());
                assert_eq!(mesh.positions.len(), mesh.uvs.len());
                assert_eq!(mesh.indices.len() % 3, 0);
                let count = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
                for &idx in &mesh.indices {
                    assert!(idx < count, "{} {lod:?}: index {idx} oob", species.name);
                }
                for &[x, y, z] in &mesh.positions {
                    assert!(x.is_finite() && y.is_finite() && z.is_finite());
                }
                for &[x, y, z] in &mesh.normals {
                    let len = (x * x + y * y + z * z).sqrt();
                    assert!(
                        (len - 1.0).abs() < 1e-3 || len < 1e-6,
                        "{}: normal not unit ({len})",
                        species.name
                    );
                }
            }
        }
    }

    #[test]
    fn coarser_lod_has_fewer_vertices() {
        // Pine 1 has trunk depth, so coarser slices markedly reduce the vertex
        // count of every trunk/branch segment.
        let Some(species) = tree_species(0) else {
            return; // species 0 is always defined
        };
        let fine = tree_geometry(species, TreeLod::Highest).positions.len();
        let coarse = tree_geometry(species, TreeLod::Low).positions.len();
        assert!(coarse < fine, "coarse {coarse} !< fine {fine}");
    }

    #[test]
    fn billboard_is_two_double_sided_quads() {
        let Some(species) = tree_species(0) else {
            return; // species 0 is always defined
        };
        let mesh = billboard_geometry(species);
        assert_eq!(mesh.positions.len(), 8);
        // Two quads, each double-sided (four triangles), so 24 indices.
        assert_eq!(mesh.indices.len(), 24);
        let count = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        for &idx in &mesh.indices {
            assert!(idx < count);
        }
    }
}
