//! Bevy integration for [`sl_tree`] Linden-tree geometry: a bridge from a
//! generated [`TreeMesh`] to Bevy's [`Mesh`], the tree counterpart of the
//! [`to_bevy_prim_mesh`](crate::to_bevy_prim_mesh) /
//! [`to_bevy_mesh`](crate::to_bevy_mesh) bridges.
//!
//! `sl-tree` is a pure, I/O-free geometry crate: a tree's branch / leaf geometry
//! is generated on the CPU from its species table entry (selected by the
//! object's `state` byte), not fetched as an asset. So — like the prim path —
//! there is nothing to drive here, only a per-tree [`Mesh`] conversion the viewer
//! feeds into `Assets<Mesh>`, plus the single species diffuse texture the app
//! pairs with it.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use sl_tree::TreeMesh;

/// Converts a generated [`TreeMesh`] into a Bevy [`Mesh`] (a `TriangleList` with
/// position, normal and UV0 attributes plus `u32` indices), ready to insert into
/// `Assets<Mesh>`.
///
/// The geometry stays in Second Life's right-handed **Z-up** space at unit outer
/// scale; the viewer applies the tree's outer scale, yaw, and its single
/// `sl_to_bevy` basis change at the entity `Transform` boundary, not here. The V
/// texture coordinate is flipped to Bevy/wgpu's top-down convention, matching
/// [`to_bevy_prim_mesh`](crate::to_bevy_prim_mesh).
#[must_use]
pub fn to_bevy_tree_mesh(tree: &TreeMesh) -> Mesh {
    let uvs: Vec<[f32; 2]> = tree.uvs.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, tree.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, tree.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(tree.indices.clone()))
}

#[cfg(test)]
mod tests {
    use super::to_bevy_tree_mesh;
    use bevy::mesh::{Indices, Mesh, VertexAttributeValues};
    use pretty_assertions::assert_eq;
    use sl_tree::{TreeLod, tree_species};

    #[test]
    fn converts_positions_normals_uvs_and_indices() {
        let Some(species) = tree_species(0) else {
            return; // species 0 is always defined
        };
        let tree = sl_tree::tree_geometry(species, TreeLod::Highest);
        let mesh = to_bevy_tree_mesh(&tree);
        // Positions carry across one-for-one.
        assert!(matches!(
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            Some(VertexAttributeValues::Float32x3(positions))
                if positions.len() == tree.positions.len()
        ));
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        // The V coordinate is flipped from the source geometry (first vertex).
        if let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
            && let (Some(got), Some(src)) = (uvs.first(), tree.uvs.first())
        {
            assert!((got[1] - (1.0 - src[1])).abs() < 1e-6);
        }
        assert_eq!(mesh.indices().map(Indices::len), Some(tree.indices.len()));
    }
}
