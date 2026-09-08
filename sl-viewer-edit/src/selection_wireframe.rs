//! The selection highlight a **skinned** face wears: a wireframe of its own
//! posed geometry.
//!
//! # Why a rigged face cannot wear the shell
//!
//! [`crate::edit_selection`]'s highlight is an **inverted-hull shell**: a second
//! draw of the face's mesh, front faces culled and pushed out by an entity
//! `Transform` scale, so only the rim shows. Skinning defeats both halves of
//! that:
//!
//! - the shell shares the face's mesh asset, so Bevy specializes it into the
//!   **skinned** pipeline (`is_skinned(layout)` — the mesh carries
//!   `JOINT_INDEX` + `JOINT_WEIGHT`) while taking the bind group from the
//!   *entity*. A shell without the skin is handed a model-only bind group, which
//!   is a wgpu validation error the render layer quits the viewer on — the same
//!   defect the waterline split had (`viewer-skinned-bind-group-quits-on-rez`);
//! - and the inflate is an entity `Transform` scale, which a skinned draw
//!   **ignores**: the vertices are placed from the joint palette, not from the
//!   entity's model matrix. Even a correctly skinned shell would sit exactly on
//!   the face rather than around it.
//!
//! # What the reference does
//!
//! It does not draw a silhouette for a mesh object at all.
//! `LLSelectNode::renderOneSilhouette` returns early for one (`vobj->isMesh()`,
//! the `SL-10194` removal), and `LLSelectMgr::renderSilhouettes` sends every mesh
//! object — rigged or not — down a separate path instead: `renderMeshSelection_f`
//! draws the selected faces as a **wireframe** (`glPolygonMode(GL_LINE)`,
//! `LLFace::renderOneWireframe`, polygon-offset toward the viewer, in the same
//! parent-yellow / child-blue silhouette colours), and for a rigged drawable it
//! first re-skins the volume (`updateRiggedVolume(true)`) so the lines follow the
//! **pose**.
//!
//! So the faithful highlight for a skinned face is a posed wireframe, and that is
//! what [`wireframe_mesh`] builds: the face's own mesh re-indexed as a
//! [`PrimitiveTopology::LineList`] over its triangle edges, carrying every vertex
//! attribute of the source — the skin weights included, so the overlay skins with
//! the face — with the positions lifted along their normals to win the depth test
//! against the surface they outline (the port of the reference's polygon offset,
//! and the one thing the entity-scale inflate can no longer do).
//!
//! Line **thickness** is not portable: `wgpu` has no line-width state, so the
//! rim is one pixel where the reference's is five. That is the whole visual
//! divergence.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use std::collections::HashSet;

/// How far the wireframe is lifted off the surface it outlines, as a fraction of
/// the mesh's own diagonal — so one constant serves an avatar-sized body and a
/// finger-sized attachment. The reference's equivalent (`silhouette_thickness`)
/// scales with view distance; a fixed fraction of the object is the cheap stand-in
/// for the one thing the lift has to achieve, which is to beat the depth test
/// against the face it hugs.
const LIFT_FRACTION: f32 = 0.004;

/// The smallest lift (metres) — a tiny mesh still has to clear the depth test.
const LIFT_MIN: f32 = 0.001;

/// The largest lift (metres) — a huge mesh must not wear its outline as a halo
/// detached from the surface.
const LIFT_MAX: f32 = 0.02;

/// Build the wireframe overlay mesh for `source`: its triangle edges as a
/// [`PrimitiveTopology::LineList`], every vertex attribute kept, the positions
/// lifted along their normals.
///
/// Keeping **every** attribute is deliberate rather than thrifty. The overlay
/// renders through the same [`FaceMaterial`](crate::face_material::FaceMaterial)
/// as the face, whose shader is compiled against the vertex layout it is handed:
/// drop the UVs and the fragment shader no longer has a `uv` to sample, drop the
/// joints and the draw stops skinning. The cost is a second vertex buffer for as
/// long as the object is selected.
///
/// Returns `None` for anything that is not an indexed triangle list, or whose
/// vertex data has already been extracted to the render world (a mesh built
/// without [`RenderAssetUsages::MAIN_WORLD`] — the caller then leaves the face
/// unhighlighted rather than guessing at its geometry).
pub(crate) fn wireframe_mesh(source: &Mesh) -> Option<Mesh> {
    if source.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let indices = source.try_indices().ok()?;
    let mut wireframe = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
    for (attribute, values) in source.try_attributes().ok()? {
        wireframe.insert_attribute(*attribute, values.clone());
    }
    lift_positions(source, &mut wireframe);
    wireframe.insert_indices(Indices::U32(triangle_edges(indices)));
    Some(wireframe)
}

/// The unique undirected edges of an indexed triangle list, as a line-list index
/// buffer.
///
/// Each interior edge is shared by two triangles, so de-duplicating roughly halves
/// the lines drawn — and, more visibly, stops each shared edge being drawn twice
/// into a translucent overlay, where a doubled line reads as a brighter one.
fn triangle_edges(indices: &Indices) -> Vec<u32> {
    // Both index widths walked as one iterator of triangles, so the body below is
    // written once and neither width is copied into a wider buffer first — a
    // rigged body's index list is hundreds of thousands of entries.
    let triangles: Box<dyn Iterator<Item = [u32; 3]> + '_> = match indices {
        Indices::U16(values) => Box::new(
            values
                .as_chunks::<3>()
                .0
                .iter()
                .map(|&[a, b, c]| [u32::from(a), u32::from(b), u32::from(c)]),
        ),
        Indices::U32(values) => Box::new(values.as_chunks::<3>().0.iter().copied()),
    };
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut edges: Vec<u32> = Vec::new();
    let mut push = |a: u32, b: u32, edges: &mut Vec<u32>| {
        let key = if a <= b { (a, b) } else { (b, a) };
        if seen.insert(key) {
            edges.push(a);
            edges.push(b);
        }
    };
    for [a, b, c] in triangles {
        push(a, b, &mut edges);
        push(b, c, &mut edges);
        push(c, a, &mut edges);
    }
    edges
}

/// Lift `wireframe`'s positions off the surface along `source`'s normals, by a
/// [`LIFT_FRACTION`] of the mesh's diagonal (clamped) — the port of the
/// reference's `glPolygonOffset(3, 3)`, done in the geometry because a skinned
/// draw has no entity-transform lever left.
///
/// The lift is applied in the mesh's own (bind-pose) space, which is where it
/// belongs: skinning is a blend of rigid joint transforms, so a bind-pose offset
/// arrives at the posed surface as the same offset, still along the posed normal.
///
/// A mesh without normals keeps its positions — an unlifted wireframe z-fights
/// its face, which is visible but not fatal, and every face this viewer builds
/// carries normals.
fn lift_positions(source: &Mesh, wireframe: &mut Mesh) {
    let Ok(Some(VertexAttributeValues::Float32x3(normals))) =
        source.try_attribute_option(Mesh::ATTRIBUTE_NORMAL)
    else {
        return;
    };
    let normals = normals.clone();
    let Ok(Some(VertexAttributeValues::Float32x3(positions))) =
        wireframe.try_attribute_mut_option(Mesh::ATTRIBUTE_POSITION)
    else {
        return;
    };
    let lift = lift_distance(positions);
    for (position, normal) in positions.iter_mut().zip(normals.iter()) {
        for axis in 0..3_usize {
            let (Some(coordinate), Some(direction)) = (position.get_mut(axis), normal.get(axis))
            else {
                continue;
            };
            *coordinate = direction.mul_add(lift, *coordinate);
        }
    }
}

/// How far to lift, from the extent of `positions`: [`LIFT_FRACTION`] of the
/// bounding box's diagonal, clamped to [`LIFT_MIN`]..=[`LIFT_MAX`].
fn lift_distance(positions: &[[f32; 3]]) -> f32 {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for position in positions {
        for axis in 0..3_usize {
            let (Some(coordinate), Some(low), Some(high)) =
                (position.get(axis), min.get_mut(axis), max.get_mut(axis))
            else {
                continue;
            };
            *low = low.min(*coordinate);
            *high = high.max(*coordinate);
        }
    }
    let mut diagonal = 0.0_f32;
    for axis in 0..3_usize {
        let (Some(low), Some(high)) = (min.get(axis), max.get(axis)) else {
            continue;
        };
        let span = high - low;
        diagonal = span.mul_add(span, diagonal);
    }
    (diagonal.sqrt() * LIFT_FRACTION).clamp(LIFT_MIN, LIFT_MAX)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        clippy::panic,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{LIFT_FRACTION, LIFT_MAX, LIFT_MIN, wireframe_mesh};
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
    use pretty_assertions::assert_eq;

    /// Two triangles sharing an edge, with normals along `+Z` and the skin
    /// attributes a rigged face carries.
    fn quad() -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [0.0_f32, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0_f32, 0.0, 1.0]; 4]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0_f32, 0.0]; 4]);
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(vec![[0_u16, 1, 2, 3]; 4]),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_WEIGHT,
            vec![[1.0_f32, 0.0, 0.0, 0.0]; 4],
        );
        mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
        mesh
    }

    /// The overlay is a line list over the **unique** edges: five for two
    /// triangles sharing one, not six — the shared edge is drawn once.
    #[test]
    fn edges_are_deduplicated() {
        let wireframe = wireframe_mesh(&quad()).expect("a triangle list yields a wireframe");
        assert_eq!(wireframe.primitive_topology(), PrimitiveTopology::LineList);
        let indices = wireframe.indices().expect("the wireframe is indexed");
        assert_eq!(indices.len(), 10, "five unique edges, two indices each");
    }

    /// The overlay keeps the skin attributes, because that is what makes it skin
    /// with the face it outlines — and dropping them is exactly the mismatch that
    /// quits the viewer.
    #[test]
    fn skin_attributes_survive() {
        let wireframe = wireframe_mesh(&quad()).expect("a triangle list yields a wireframe");
        assert!(
            wireframe.contains_attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
                && wireframe.contains_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT),
            "the wireframe must specialize the same (skinned) pipeline as its face"
        );
        assert!(
            wireframe.contains_attribute(Mesh::ATTRIBUTE_UV_0),
            "the face shader is compiled against the face's own vertex layout"
        );
    }

    /// The positions are lifted along the normals, so the lines sit in front of
    /// the surface instead of z-fighting it. The quad's normals are `+Z` and its
    /// diagonal is `√2`, so every vertex moves `√2 × LIFT_FRACTION` along `+Z` —
    /// between the floor and the ceiling, so neither clamp is in play.
    #[test]
    fn positions_are_lifted_along_the_normal() {
        let wireframe = wireframe_mesh(&quad()).expect("a triangle list yields a wireframe");
        let Some(VertexAttributeValues::Float32x3(positions)) =
            wireframe.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the wireframe carries positions");
        };
        let expected = 2.0_f32.sqrt() * LIFT_FRACTION;
        assert!(
            (LIFT_MIN..=LIFT_MAX).contains(&expected),
            "the fixture is meant to exercise the unclamped lift"
        );
        for position in positions {
            let z = position
                .get(2)
                .copied()
                .expect("a three-component position");
            assert!(
                (z - expected).abs() < 1e-6,
                "every vertex is lifted {expected} along +Z, got {z}"
            );
        }
    }

    /// Anything that is not an indexed triangle list has no edges to walk — the
    /// caller leaves such a face unhighlighted rather than guessing.
    #[test]
    fn non_triangle_input_is_refused() {
        let mut lines = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
        lines.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0_f32, 0.0, 0.0]; 2]);
        lines.insert_indices(Indices::U32(vec![0, 1]));
        assert!(wireframe_mesh(&lines).is_none());

        let mut unindexed = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        unindexed.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0_f32, 0.0, 0.0]; 3]);
        assert!(wireframe_mesh(&unindexed).is_none());
    }
}
