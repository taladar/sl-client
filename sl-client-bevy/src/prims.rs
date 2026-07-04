//! Bevy integration for [`sl_prim`] prim tessellation: a bridge from a
//! tessellated [`PrimFace`] to Bevy's [`Mesh`], the prim counterpart of the
//! [`to_bevy_mesh`](crate::to_bevy_mesh) decoded-mesh bridge.
//!
//! `sl-prim` is a pure, I/O-free geometry crate (no store, no fetcher): a prim's
//! geometry is computed on the CPU from its shape parameters (which arrive in an
//! `ObjectUpdate`), not fetched as a separate asset. So — unlike the texture /
//! mesh / asset stores — there is nothing to drive here, only a per-face
//! [`Mesh`] conversion the viewer feeds into `Assets<Mesh>`.
//!
//! Only geometry is bridged: pairing each face to its material (the per-face
//! `TextureEntry` slot named by [`PrimFace::face_id`]) is the app's job, exactly
//! as with a decoded mesh.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use sl_prim::{PrimFace, PrimMesh};

/// Converts one tessellated [`PrimFace`] into a Bevy [`Mesh`] (a `TriangleList`
/// with position, and — when present — normal and UV0 attributes plus `u32`
/// indices), ready to insert into `Assets<Mesh>`.
///
/// The face's geometry stays in Second Life's right-handed **Z-up** space; the
/// viewer applies its single `sl_to_bevy` conversion at the entity `Transform`
/// boundary, not here.
#[must_use]
pub fn to_bevy_prim_mesh(face: &PrimFace) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, face.positions.clone());
    if !face.normals.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, face.normals.clone());
    }
    if !face.uvs.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, face.uvs.clone());
    }
    mesh.insert_indices(Indices::U32(face.indices.clone()));
    mesh
}

/// Converts a whole tessellated prim into one Bevy [`Mesh`] per face (skipping
/// empty faces that carry no geometry), preserving face order so the app can
/// pair each with the per-face material named by its
/// [`face_id`](PrimFace::face_id).
#[must_use]
pub fn to_bevy_prim_meshes(prim: &PrimMesh) -> Vec<Mesh> {
    prim.faces
        .iter()
        .filter(|face| !face.is_empty())
        .map(to_bevy_prim_mesh)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{to_bevy_prim_mesh, to_bevy_prim_meshes};
    use bevy::mesh::{Mesh, VertexAttributeValues};
    use pretty_assertions::assert_eq;
    use sl_prim::{PrimFace, PrimFaceId, PrimMesh};

    /// A single-triangle face with positions, normals, UVs, and indices.
    fn triangle() -> PrimFace {
        let mut face = PrimFace::empty(PrimFaceId::new(0));
        face.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        face.normals = vec![[0.0, -1.0, 0.0], [0.0, -1.0, 0.0], [0.0, -1.0, 0.0]];
        face.uvs = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        face.indices = vec![0, 1, 2];
        face
    }

    #[test]
    fn builds_a_bevy_mesh_from_a_face() {
        let mesh = to_bevy_prim_mesh(&triangle());
        // Positions attribute has the three vertices.
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION);
        assert!(matches!(
            positions,
            Some(VertexAttributeValues::Float32x3(values)) if values.len() == 3
        ));
        // The normal and UV0 attributes are present too.
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        // Three indices form one triangle.
        assert_eq!(mesh.indices().map(bevy::mesh::Indices::len), Some(3));
    }

    #[test]
    fn omits_absent_attributes() {
        // A position-only face (no normals, no UVs) yields a mesh with neither.
        let mut face = PrimFace::empty(PrimFaceId::new(0));
        face.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        face.indices = vec![0, 1, 2];
        let mesh = to_bevy_prim_mesh(&face);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_none());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_none());
    }

    #[test]
    fn skips_empty_faces() {
        let prim = PrimMesh {
            faces: vec![triangle(), PrimFace::empty(PrimFaceId::new(1))],
        };
        // Only the real face becomes a Bevy mesh.
        assert_eq!(to_bevy_prim_meshes(&prim).len(), 1);
    }
}
