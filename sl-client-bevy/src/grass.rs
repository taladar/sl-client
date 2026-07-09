//! Bevy integration for [`sl_tree`] `LLVOGrass` geometry: a bridge from a
//! generated [`GrassMesh`] to Bevy's [`Mesh`], the grass counterpart of
//! [`to_bevy_tree_mesh`](crate::to_bevy_tree_mesh).
//!
//! As with the tree path `sl-tree` is a pure, I/O-free geometry crate: a grass
//! clump's blade geometry is generated on the CPU from its species table entry
//! (selected by the object's `state` byte) and the object scale, not fetched as an
//! asset. So there is nothing to drive here, only a per-object [`Mesh`] conversion
//! the viewer feeds into `Assets<Mesh>`, plus the single species diffuse texture
//! the app pairs with it.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use sl_tree::GrassMesh;

/// Converts a generated [`GrassMesh`] into a Bevy [`Mesh`] (a `TriangleList` with
/// position, normal and UV0 attributes plus `u32` indices), ready to insert into
/// `Assets<Mesh>`.
///
/// The geometry stays in Second Life's right-handed **Z-up** space in absolute
/// metres (the object scale is already folded into the blade-centre spread); the
/// viewer applies the object's position / rotation and its single `sl_to_bevy`
/// basis change at the entity `Transform` boundary, not here. The V texture
/// coordinate is flipped to Bevy/wgpu's top-down convention, matching
/// [`to_bevy_tree_mesh`](crate::to_bevy_tree_mesh).
#[must_use]
pub fn to_bevy_grass_mesh(grass: &GrassMesh) -> Mesh {
    let uvs: Vec<[f32; 2]> = grass.uvs.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, grass.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, grass.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(grass.indices.clone()))
}

#[cfg(test)]
mod tests {
    use super::to_bevy_grass_mesh;
    use bevy::mesh::{Indices, Mesh, VertexAttributeValues};
    use pretty_assertions::assert_eq;
    use sl_tree::{GRASS_MAX_BLADES, grass_geometry, grass_species};

    #[test]
    fn converts_positions_normals_uvs_and_indices() {
        let Some(species) = grass_species(0) else {
            return; // species 0 is always defined
        };
        let grass = grass_geometry(species, 1.0, 1.0, GRASS_MAX_BLADES);
        let mesh = to_bevy_grass_mesh(&grass);
        // Positions carry across one-for-one.
        assert!(matches!(
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            Some(VertexAttributeValues::Float32x3(positions))
                if positions.len() == grass.positions.len()
        ));
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        // The V coordinate is flipped from the source geometry (first vertex).
        if let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
            && let (Some(got), Some(src)) = (uvs.first(), grass.uvs.first())
        {
            assert!((got[1] - (1.0 - src[1])).abs() < 1e-6);
        }
        assert_eq!(mesh.indices().map(Indices::len), Some(grass.indices.len()));
    }
}
