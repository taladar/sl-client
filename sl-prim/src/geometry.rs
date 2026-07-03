//! The geometry output types of prim tessellation.
//!
//! A tessellated prim is a [`PrimMesh`]: an ordered list of [`PrimFace`]s, each
//! one drawable face carrying its own dequantized positions, normals, texture
//! coordinates, and triangle-list indices in Second Life's right-handed
//! **Z-up** space. This mirrors [`sl_mesh::DecodedMesh`] / [`sl_mesh::Submesh`]
//! so the `to_bevy_prim_mesh` conversion (in `sl-client-bevy`) can reuse the
//! same per-face-entity rendering path as decoded meshes.
//!
//! [`sl_mesh::DecodedMesh`]: https://docs.rs/sl-mesh
//! [`sl_mesh::Submesh`]: https://docs.rs/sl-mesh

/// The Linden semantic face index of a [`PrimFace`]: the texture-entry (`TE`)
/// slot this face is textured from. It is the sequential render-face number the
/// simulator assigns a volume's faces (`0` upward), **not** the internal
/// `LL_FACE_*` bit flag; a viewer looks the face's texture up by this index
/// (`TextureEntry.faces[face_id]`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct PrimFaceId(u16);

impl PrimFaceId {
    /// Wraps a raw Linden face index.
    #[must_use]
    pub const fn new(index: u16) -> Self {
        Self(index)
    }

    /// The raw Linden face index.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// The face index widened to `usize`, for indexing a texture-entry face
    /// list.
    #[must_use]
    pub fn as_usize(self) -> usize {
        usize::from(self.0)
    }
}

impl From<u16> for PrimFaceId {
    /// Wraps a raw Linden face index.
    fn from(index: u16) -> Self {
        Self(index)
    }
}

/// One drawable face of a tessellated prim: dequantized geometry in the prim's
/// local, right-handed **Z-up** space plus the [`PrimFaceId`] naming which
/// texture-entry slot textures it.
///
/// The four vertex arrays are parallel (one entry per vertex); [`indices`] is a
/// flat triangle list (a multiple of three) indexing into them. A default
/// [`PrimFace`] is empty — a placeholder carrying no geometry.
///
/// [`indices`]: PrimFace::indices
#[derive(Clone, Debug, Default)]
pub struct PrimFace {
    /// Vertex positions in the prim's local, right-handed Z-up space.
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex normals, parallel to [`positions`](Self::positions) (empty if
    /// the face carries none).
    pub normals: Vec<[f32; 3]>,
    /// Per-vertex UV0 texture coordinates, parallel to
    /// [`positions`](Self::positions) (empty if the face carries none).
    pub uvs: Vec<[f32; 2]>,
    /// Triangle-list indices into the vertex arrays (a multiple of three).
    pub indices: Vec<u32>,
    /// The Linden semantic face index this face is textured from.
    pub face_id: PrimFaceId,
}

impl PrimFace {
    /// An empty face textured from `face_id` (no vertices, no triangles). Used
    /// as the starting point a tessellator appends into, and as the degenerate
    /// fallback where a face cannot be built.
    #[must_use]
    pub const fn empty(face_id: PrimFaceId) -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            face_id,
        }
    }

    /// The number of vertices in this face.
    #[must_use]
    pub const fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// The number of triangles in this face (its index count divided by three).
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len().checked_div(3).unwrap_or(0)
    }

    /// Whether this face carries no geometry.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// A fully tessellated prim: its faces, in Linden face order.
///
/// This is the output of sweeping a 2D profile ring along an extrusion path and
/// capping the ends (the later `sl-prim` phases); one [`PrimFace`] per drawable
/// face so each can carry its own material downstream.
#[derive(Clone, Debug, Default)]
pub struct PrimMesh {
    /// The prim's drawable faces, in Linden face order.
    pub faces: Vec<PrimFace>,
}

impl PrimMesh {
    /// An empty prim (no faces).
    #[must_use]
    pub const fn new() -> Self {
        Self { faces: Vec::new() }
    }

    /// The number of faces.
    #[must_use]
    pub const fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// The total vertex count across every face.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.faces.iter().fold(0_usize, |total, face| {
            total.saturating_add(face.vertex_count())
        })
    }

    /// The total triangle count across every face.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.faces.iter().fold(0_usize, |total, face| {
            total.saturating_add(face.triangle_count())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{PrimFace, PrimFaceId, PrimMesh};
    use pretty_assertions::assert_eq;

    #[test]
    fn face_id_round_trips_through_u16() {
        let id = PrimFaceId::new(4);
        assert_eq!(id.get(), 4);
        assert_eq!(id.as_usize(), 4);
        assert_eq!(PrimFaceId::from(7).get(), 7);
    }

    #[test]
    fn empty_face_has_no_geometry() {
        let face = PrimFace::empty(PrimFaceId::new(2));
        assert!(face.is_empty());
        assert_eq!(face.vertex_count(), 0);
        assert_eq!(face.triangle_count(), 0);
        assert_eq!(face.face_id, PrimFaceId::new(2));
    }

    #[test]
    fn triangle_count_divides_indices_by_three() {
        let mut face = PrimFace::empty(PrimFaceId::default());
        face.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        face.indices = vec![0, 1, 2];
        assert_eq!(face.vertex_count(), 3);
        assert_eq!(face.triangle_count(), 1);
        assert!(!face.is_empty());
    }

    #[test]
    fn mesh_totals_sum_over_faces() {
        let mut a = PrimFace::empty(PrimFaceId::new(0));
        a.positions = vec![[0.0; 3]; 4];
        a.indices = vec![0, 1, 2, 0, 2, 3];
        let mut b = PrimFace::empty(PrimFaceId::new(1));
        b.positions = vec![[0.0; 3]; 3];
        b.indices = vec![0, 1, 2];
        let mesh = PrimMesh { faces: vec![a, b] };
        assert_eq!(mesh.face_count(), 2);
        assert_eq!(mesh.vertex_count(), 7);
        assert_eq!(mesh.triangle_count(), 3);
    }

    #[test]
    fn default_mesh_is_empty() {
        let mesh = PrimMesh::new();
        assert_eq!(mesh.face_count(), 0);
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.triangle_count(), 0);
    }
}
