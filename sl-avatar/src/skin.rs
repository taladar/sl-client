//! Rigged-mesh skinning math (P17.1): the matrix-palette skin that deforms a
//! rigged `sl_mesh` body/clothing with an avatar's posed skeleton.
//!
//! A rigged mesh ships a [`MeshSkin`] block — the rig's joint names, one
//! inverse-bind matrix per joint, a single bind-shape matrix — and every vertex
//! carries up to four [`VertexWeights`] `(joint, weight)` influences. To skin a
//! vertex the viewer builds a **matrix palette**: for each rig joint `j`,
//!
//! ```text
//! palette[j] = inverse_bind_matrix[j] * joint_world_matrix[j]
//! ```
//!
//! where `joint_world_matrix[j]` is the *current* world transform of the avatar
//! skeleton joint the rig joint `joint_names[j]` binds to (the "skeleton
//! instance" the caller poses). A vertex at mesh-local position `v` with
//! normalized influences `w_k` on joints `idx_k` then lands at
//!
//! ```text
//! v' = (v * bind_shape_matrix) * Σ_k w_k * palette[idx_k]
//! ```
//!
//! This mirrors Firestorm's `LLSkinningUtil::initSkinningMatrixPalette` +
//! `getPerVertexSkinMatrix` + `LLVOVolume::updateRiggedVolume` (bind-shape
//! transform, then the weight-blended palette matrix). All matrices follow
//! Second Life's **row-vector, row-major** convention (`v * M`, translation in
//! the last row) — the same layout `sl_mesh` decodes into `[f32; 16]` — and stay
//! in Second Life's right-handed Z-up metre space. This module is pure and
//! Bevy-free; the caller (`sl-client-bevy`, P17.2) supplies the joint world
//! transforms from a posed skeleton instance and consumes the skinned vertices.
//!
//! The [`MeshSkin`] `alt_inverse_bind_matrix`, `pelvis_offset`, and
//! `lock_scale_if_joint_position` fields are consumed **upstream**, when the
//! caller builds the skeleton instance's joint world transforms (joint-position
//! overrides and the pelvis fixup): they shape the `joint_world_matrix` inputs
//! rather than the palette algebra here, so this module only reads
//! `joint_names`, `inverse_bind_matrix`, and `bind_shape_matrix`.

use sl_mesh::{MeshSkin, VertexWeights};

/// A 4×4 transform in Second Life's row-major, row-vector convention: 16 floats
/// laid out row by row (`m[row * 4 + col]`), applied to a point as `v * M` with
/// the translation in the last row. Identical to `sl_mesh`'s matrix storage.
type Mat4 = [f32; 16];

/// The 4×4 identity in [`Mat4`] layout.
const IDENTITY: Mat4 = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
];

/// A built matrix palette for one rigged mesh skin: the per-joint skinning
/// matrices plus the skin's bind-shape matrix, ready to deform that mesh's
/// vertices.
///
/// Build it once per `(skin, current pose)` with [`SkinningPalette::build`],
/// then call [`skin_position`](Self::skin_position) /
/// [`skin_normal`](Self::skin_normal) per vertex.
#[derive(Clone, Debug)]
pub struct SkinningPalette {
    /// One skinning matrix per rig joint, in `skin.joint_names` order:
    /// `inverse_bind_matrix[j] * joint_world_matrix[j]`.
    matrices: Vec<Mat4>,
    /// The skin's bind-shape matrix, applied to every vertex before the blended
    /// palette matrix.
    bind_shape: Mat4,
}

impl SkinningPalette {
    /// Build the palette for `skin` against a posed skeleton instance.
    ///
    /// `joint_world` resolves a rig joint's name (from `skin.joint_names`) to
    /// that avatar joint's **current world transform** as 16 row-major floats
    /// (`[f32; 16]`, SL's row-vector convention). A
    /// name it cannot resolve falls back to the identity world transform, so
    /// that joint's palette entry becomes its bare inverse-bind matrix — the
    /// same fallback the reference viewer uses for a joint it cannot find
    /// (`initSkinningMatrixPalette`). A joint with no inverse-bind matrix (a
    /// malformed skin) falls back to identity.
    pub fn build<F>(skin: &MeshSkin, mut joint_world: F) -> Self
    where
        F: FnMut(&str) -> Option<Mat4>,
    {
        let matrices = skin
            .joint_names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let inverse_bind = skin
                    .inverse_bind_matrix
                    .get(index)
                    .copied()
                    .unwrap_or(IDENTITY);
                let world = joint_world(name).unwrap_or(IDENTITY);
                mat_mul(&inverse_bind, &world)
            })
            .collect();
        Self {
            matrices,
            bind_shape: skin.bind_shape_matrix,
        }
    }

    /// The number of joints in the palette.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.matrices.len()
    }

    /// Whether the palette has no joints.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.matrices.is_empty()
    }

    /// Skin one mesh-local vertex position into world space.
    ///
    /// Applies the bind-shape matrix, then the weight-blended palette matrix.
    /// Influences that reference an out-of-range joint are ignored; the
    /// remaining weights are renormalized to sum to one (matching the reference
    /// viewer's per-vertex weight normalization). A vertex with no usable
    /// influence is placed by the bind-shape matrix alone.
    #[must_use]
    pub fn skin_position(&self, position: [f32; 3], weights: &VertexWeights) -> [f32; 3] {
        let bound = transform_point(&self.bind_shape, position);
        match self.blended_matrix(weights) {
            Some(skin) => transform_point(&skin, bound),
            None => bound,
        }
    }

    /// Skin one mesh-local vertex normal, returning a renormalized world-space
    /// normal.
    ///
    /// Uses the linear (rotation/scale) part of the bind-shape and blended
    /// palette matrices — translation does not apply to a direction. This is the
    /// reference viewer's standard approximation; it is exact for rigid and
    /// uniform-scale joint transforms.
    #[must_use]
    pub fn skin_normal(&self, normal: [f32; 3], weights: &VertexWeights) -> [f32; 3] {
        let bound = transform_direction(&self.bind_shape, normal);
        let skinned = match self.blended_matrix(weights) {
            Some(skin) => transform_direction(&skin, bound),
            None => bound,
        };
        normalize(skinned)
    }

    /// The weight-blended, weight-normalized skinning matrix for one vertex, or
    /// `None` if no influence references a valid joint.
    fn blended_matrix(&self, weights: &VertexWeights) -> Option<Mat4> {
        let mut accumulated = [0.0_f32; 16];
        let mut total = 0.0_f32;
        for &(joint, weight) in &weights.influences {
            let Some(matrix) = self.matrices.get(usize::from(joint)) else {
                continue;
            };
            add_scaled(&mut accumulated, matrix, weight);
            total += weight;
        }
        if total <= 0.0 {
            return None;
        }
        let inverse_total = 1.0 / total;
        for element in &mut accumulated {
            *element *= inverse_total;
        }
        Some(accumulated)
    }
}

/// One element of a [`Mat4`] at `row`, `col` (both `0..4`), or `0.0` if either
/// is out of range. Keeps the matrix helpers off denied slice indexing without a
/// panic path.
fn at(m: &Mat4, row: usize, col: usize) -> f32 {
    let index = row.checked_mul(4).and_then(|base| base.checked_add(col));
    index.and_then(|index| m.get(index)).copied().unwrap_or(0.0)
}

/// Multiply two [`Mat4`]s in row-major order: `out` such that `v * out ==
/// (v * a) * b` for a row vector `v` (`out[i][j] = Σ_k a[i][k] * b[k][j]`).
fn mat_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let cell = |row: usize, col: usize| {
        at(a, row, 0) * at(b, 0, col)
            + at(a, row, 1) * at(b, 1, col)
            + at(a, row, 2) * at(b, 2, col)
            + at(a, row, 3) * at(b, 3, col)
    };
    [
        cell(0, 0),
        cell(0, 1),
        cell(0, 2),
        cell(0, 3),
        cell(1, 0),
        cell(1, 1),
        cell(1, 2),
        cell(1, 3),
        cell(2, 0),
        cell(2, 1),
        cell(2, 2),
        cell(2, 3),
        cell(3, 0),
        cell(3, 1),
        cell(3, 2),
        cell(3, 3),
    ]
}

/// Transform a point (row vector, `w = 1`) by a [`Mat4`], applying the
/// translation in the matrix's last row.
fn transform_point(m: &Mat4, point: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = point;
    [
        x * at(m, 0, 0) + y * at(m, 1, 0) + z * at(m, 2, 0) + at(m, 3, 0),
        x * at(m, 0, 1) + y * at(m, 1, 1) + z * at(m, 2, 1) + at(m, 3, 1),
        x * at(m, 0, 2) + y * at(m, 1, 2) + z * at(m, 2, 2) + at(m, 3, 2),
    ]
}

/// Transform a direction (row vector, `w = 0`) by a [`Mat4`], ignoring the
/// matrix's translation row.
fn transform_direction(m: &Mat4, direction: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = direction;
    [
        x * at(m, 0, 0) + y * at(m, 1, 0) + z * at(m, 2, 0),
        x * at(m, 0, 1) + y * at(m, 1, 1) + z * at(m, 2, 1),
        x * at(m, 0, 2) + y * at(m, 1, 2) + z * at(m, 2, 2),
    ]
}

/// Add `scale * source` into `target` component-wise.
fn add_scaled(target: &mut [f32; 16], source: &Mat4, scale: f32) {
    for (slot, &value) in target.iter_mut().zip(source.iter()) {
        *slot += value * scale;
    }
}

/// Normalize a vector to unit length, returning it unchanged if it is
/// degenerate (zero length).
fn normalize(vector: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = vector;
    let length_squared = x * x + y * y + z * z;
    if length_squared <= 0.0 {
        return vector;
    }
    let inverse_length = 1.0 / length_squared.sqrt();
    [x * inverse_length, y * inverse_length, z * inverse_length]
}

#[cfg(test)]
mod tests {
    use super::{IDENTITY, Mat4, SkinningPalette};
    use pretty_assertions::assert_eq;
    use sl_mesh::{MeshSkin, VertexWeights};

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A row-major, row-vector translation matrix.
    fn translation(x: f32, y: f32, z: f32) -> Mat4 {
        [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            x, y, z, 1.0, //
        ]
    }

    /// A skin over `joints` with identity inverse-bind and bind-shape matrices.
    fn identity_skin(joints: &[&str]) -> MeshSkin {
        MeshSkin {
            joint_names: joints.iter().map(|name| (*name).to_owned()).collect(),
            inverse_bind_matrix: joints.iter().map(|_| IDENTITY).collect(),
            bind_shape_matrix: IDENTITY,
            alt_inverse_bind_matrix: Vec::new(),
            pelvis_offset: None,
            lock_scale_if_joint_position: false,
        }
    }

    /// One influence, fully weighted onto a single joint.
    fn single(joint: u8) -> VertexWeights {
        VertexWeights {
            influences: vec![(joint, 1.0)],
        }
    }

    /// Compare two vectors within a tolerance (keeps the assertion off `float_cmp`).
    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1.0e-4)
    }

    #[test]
    fn single_joint_translation_moves_the_vertex() -> Result<(), TestError> {
        let skin = identity_skin(&["mPelvis", "mTorso"]);
        // mPelvis translates +10 X, mTorso +10 Y.
        let palette = SkinningPalette::build(&skin, |name| match name {
            "mPelvis" => Some(translation(10.0, 0.0, 0.0)),
            "mTorso" => Some(translation(0.0, 10.0, 0.0)),
            _ => None,
        });
        assert!(close(
            palette.skin_position([0.0, 0.0, 0.0], &single(0)),
            [10.0, 0.0, 0.0]
        ));
        assert!(close(
            palette.skin_position([0.0, 0.0, 0.0], &single(1)),
            [0.0, 10.0, 0.0]
        ));
        Ok(())
    }

    #[test]
    fn blended_weights_interpolate_and_normalize() -> Result<(), TestError> {
        let skin = identity_skin(&["a", "b"]);
        let palette = SkinningPalette::build(&skin, |name| match name {
            "a" => Some(translation(10.0, 0.0, 0.0)),
            "b" => Some(translation(0.0, 10.0, 0.0)),
            _ => None,
        });
        // Equal weights -> midpoint.
        let even = VertexWeights {
            influences: vec![(0, 0.5), (1, 0.5)],
        };
        assert!(close(
            palette.skin_position([0.0, 0.0, 0.0], &even),
            [5.0, 5.0, 0.0]
        ));
        // Unnormalized weights (sum 0.5) renormalize to the same midpoint.
        let scaled = VertexWeights {
            influences: vec![(0, 0.25), (1, 0.25)],
        };
        assert!(close(
            palette.skin_position([0.0, 0.0, 0.0], &scaled),
            [5.0, 5.0, 0.0]
        ));
        // 3:1 weighting biases toward joint a.
        let biased = VertexWeights {
            influences: vec![(0, 0.75), (1, 0.25)],
        };
        assert!(close(
            palette.skin_position([0.0, 0.0, 0.0], &biased),
            [7.5, 2.5, 0.0]
        ));
        Ok(())
    }

    #[test]
    fn inverse_bind_cancels_the_bind_world_transform() -> Result<(), TestError> {
        // A joint whose world transform is the inverse of its bind pose leaves a
        // bound vertex where it started (palette entry = invBind * world = I).
        let skin = MeshSkin {
            joint_names: vec!["j".to_owned()],
            inverse_bind_matrix: vec![translation(-2.0, 0.0, 0.0)],
            bind_shape_matrix: IDENTITY,
            alt_inverse_bind_matrix: Vec::new(),
            pelvis_offset: None,
            lock_scale_if_joint_position: false,
        };
        let palette = SkinningPalette::build(&skin, |_| Some(translation(2.0, 0.0, 0.0)));
        assert!(close(
            palette.skin_position([1.0, 2.0, 3.0], &single(0)),
            [1.0, 2.0, 3.0]
        ));
        Ok(())
    }

    #[test]
    fn bind_shape_applies_before_the_palette() -> Result<(), TestError> {
        let mut skin = identity_skin(&["j"]);
        skin.bind_shape_matrix = translation(1.0, 0.0, 0.0);
        // Identity world -> only the bind-shape shifts the vertex.
        let palette = SkinningPalette::build(&skin, |_| Some(IDENTITY));
        assert!(close(
            palette.skin_position([0.0, 0.0, 0.0], &single(0)),
            [1.0, 0.0, 0.0]
        ));
        // Bind shape (+1 X) then a joint world of +10 Y compose.
        let palette = SkinningPalette::build(&skin, |_| Some(translation(0.0, 10.0, 0.0)));
        assert!(close(
            palette.skin_position([0.0, 0.0, 0.0], &single(0)),
            [1.0, 10.0, 0.0]
        ));
        Ok(())
    }

    #[test]
    fn missing_joint_falls_back_to_inverse_bind() -> Result<(), TestError> {
        // World resolver returns None -> world = identity -> palette entry is the
        // bare inverse-bind matrix (a -3 X translation here).
        let skin = MeshSkin {
            joint_names: vec!["ghost".to_owned()],
            inverse_bind_matrix: vec![translation(-3.0, 0.0, 0.0)],
            bind_shape_matrix: IDENTITY,
            alt_inverse_bind_matrix: Vec::new(),
            pelvis_offset: None,
            lock_scale_if_joint_position: false,
        };
        let palette = SkinningPalette::build(&skin, |_| None);
        assert!(close(
            palette.skin_position([0.0, 0.0, 0.0], &single(0)),
            [-3.0, 0.0, 0.0]
        ));
        Ok(())
    }

    #[test]
    fn out_of_range_and_empty_influences_are_safe() -> Result<(), TestError> {
        let skin = identity_skin(&["a"]);
        let palette = SkinningPalette::build(&skin, |_| Some(translation(10.0, 0.0, 0.0)));
        // An influence naming a non-existent joint index is ignored; with no
        // valid influence the vertex is placed by the bind shape alone (identity).
        let bad = VertexWeights {
            influences: vec![(9, 1.0)],
        };
        assert!(close(
            palette.skin_position([4.0, 5.0, 6.0], &bad),
            [4.0, 5.0, 6.0]
        ));
        // A totally empty influence list is likewise bind-shape only.
        let empty = VertexWeights {
            influences: Vec::new(),
        };
        assert!(close(
            palette.skin_position([4.0, 5.0, 6.0], &empty),
            [4.0, 5.0, 6.0]
        ));
        Ok(())
    }

    #[test]
    fn normals_rotate_without_translating() -> Result<(), TestError> {
        // A +90° rotation about Z (row-vector convention): X -> Y.
        let rotate_z_90: Mat4 = [
            0.0, 1.0, 0.0, 0.0, //
            -1.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            5.0, 0.0, 0.0, 1.0, // translation must NOT affect the normal
        ];
        let skin = identity_skin(&["j"]);
        let palette = SkinningPalette::build(&skin, |_| Some(rotate_z_90));
        let normal = palette.skin_normal([1.0, 0.0, 0.0], &single(0));
        assert!(close(normal, [0.0, 1.0, 0.0]));
        // Still unit length.
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        assert!((length - 1.0).abs() < 1.0e-4);
        Ok(())
    }

    #[test]
    fn palette_len_tracks_joint_count() -> Result<(), TestError> {
        let skin = identity_skin(&["a", "b", "c"]);
        let palette = SkinningPalette::build(&skin, |_| Some(IDENTITY));
        assert_eq!(palette.len(), 3);
        assert!(!palette.is_empty());
        Ok(())
    }
}
