//! The selection highlight a **mesh object's** face wears: a wireframe of its
//! own geometry, posed with it when it is skinned.
//!
//! # Why a rigged face cannot wear the silhouette
//!
//! [`crate::edit_selection`]'s other highlight is the **silhouette ribbon**
//! ([`crate::selection_silhouette`]): the face's view-dependent silhouette edges,
//! each widened into a quad standing off the surface along its own normals.
//! Which edges those are is a property of the geometry **as drawn**, and a rigged
//! face's drawn geometry exists only in the GPU joint palette: the mesh asset
//! holds the bind pose, so a ribbon derived from it would trace the silhouette of
//! a T-posed limb and then skin that stale edge set onto the posed one. The
//! reference solves this by re-skinning the volume on the CPU first
//! (`updateRiggedVolume(true)`) — and then wireframes it anyway, because the
//! split is by object kind (below) and every rigged face in this viewer belongs
//! to a mesh object.
//!
//! A wireframe has no such problem: **every** edge is drawn, so there is nothing
//! view-dependent to go stale, and the lines skin with the face because the
//! derived mesh keeps the face's own joint attributes.
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
//! The split is by **object kind**, not by rigging: `isMesh()` is the sculpt
//! block's stitching type (`LL_SCULPT_TYPE_MESH`), so an ordinary uploaded mesh
//! object is wireframed exactly like an animesh, while prims, sculpts, trees and
//! grass keep the silhouette path. This viewer's equivalent predicate is
//! [`ObjectCategory::Mesh`](crate::objects::ObjectCategory::Mesh), and a rigged
//! face is wireframed on top of that because a bind-pose silhouette is the wrong
//! edge set for it (above).
//!
//! So the faithful highlight for a mesh face is a wireframe, and that is what
//! [`wireframe_mesh`] builds: the face's own mesh re-indexed as a
//! [`PrimitiveTopology::LineList`] over its triangle edges, carrying every vertex
//! attribute of the source — the skin weights included, so a rigged overlay skins
//! with the face — with the positions lifted along their normals to win the depth
//! test against the surface they outline (the port of the reference's polygon
//! offset, done in the geometry because a skinned draw places its vertices from
//! the joint palette and ignores the entity transform entirely).
//!
//! Line **thickness** is not portable: `wgpu` has no line-width state, so the
//! rim is one pixel where the reference's is five. That is the whole visual
//! divergence.

use bevy::asset::RenderAssetUsages;
use bevy::math::Vec3;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use std::collections::HashSet;

/// How far the wireframe is lifted off the surface it outlines, as a fraction of
/// the object's own diagonal **in metres** — so one constant serves an
/// avatar-sized body and a finger-sized attachment. The reference's equivalent
/// (`silhouette_thickness`) scales with view distance; a fixed fraction of the
/// object is the cheap stand-in for the one thing the lift has to achieve, which
/// is to beat the depth test against the face it hugs.
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
/// `scale` is how many metres one unit of the mesh's own space is worth on each
/// axis — the entity scale the overlay will be drawn under. A rigged face's
/// geometry is already in metres and its entity transform is ignored by a skinned
/// draw, so that caller passes [`Vec3::ONE`]; an unrigged mesh object's geometry
/// is in the asset's normalized space with the object's Second Life scale on its
/// geometry holder, so that caller passes the holder's scale. Without it the lift
/// clamps below — which are metres — would be applied to normalized units, and a
/// 20-metre mesh object would wear its outline as a hand's-width halo.
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
pub(crate) fn wireframe_mesh(source: &Mesh, scale: Vec3) -> Option<Mesh> {
    if source.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let indices = source.try_indices().ok()?;
    let mut wireframe = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
    for (attribute, values) in source.try_attributes().ok()? {
        wireframe.insert_attribute(*attribute, values.clone());
    }
    lift_positions(source, &mut wireframe, scale);
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
/// [`LIFT_FRACTION`] of the object's diagonal (clamped) — the port of the
/// reference's `glPolygonOffset(3, 3)`, done in the geometry because a skinned
/// draw has no entity-transform lever left and because an entity scale would
/// inflate an unrigged wireframe away from the surface instead of hugging it.
///
/// The lift is applied in the mesh's own (bind-pose) space, which is where it
/// belongs: skinning is a blend of rigid joint transforms, so a bind-pose offset
/// arrives at the posed surface as the same offset, still along the posed normal.
///
/// `scale` (metres per mesh unit, per axis) makes the lift a **world** distance
/// rather than a local one: each vertex moves `lift / |scale · normal|` locally,
/// which the entity scale then stretches back to `lift` metres along that normal.
/// A uniform unit scale leaves the whole computation exactly as it was.
///
/// A mesh without normals keeps its positions — an unlifted wireframe z-fights
/// its face, which is visible but not fatal, and every face this viewer builds
/// carries normals.
fn lift_positions(source: &Mesh, wireframe: &mut Mesh, scale: Vec3) {
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
    let lift = lift_distance(positions, scale);
    for (position, normal) in positions.iter_mut().zip(normals.iter()) {
        let stretch = normal_stretch(*normal, scale);
        if stretch <= f32::EPSILON {
            continue;
        }
        let local = lift / stretch;
        for axis in 0..3_usize {
            let (Some(coordinate), Some(direction)) = (position.get_mut(axis), normal.get(axis))
            else {
                continue;
            };
            *coordinate = direction.mul_add(local, *coordinate);
        }
    }
}

/// How far a local step of one `normal` carries **in metres** under `scale`
/// (metres per mesh unit, per axis) — the divisor that turns a world distance
/// along a normal into the local offset that draws as that distance.
///
/// A normal along a flattened axis is shortened by that axis's scale and a normal
/// across it is not, so this is per-vertex rather than one factor for the mesh.
/// Using the normal as stored (rather than a normalized copy) makes the division
/// exact for any normal length. Component-wise, because the glam [`Vec3`]
/// operators trip the workspace `arithmetic_side_effects` lint.
///
/// Shared with [`crate::selection_silhouette`], whose ribbon stands off the
/// surface by the same kind of world distance.
pub(crate) fn normal_stretch(normal: [f32; 3], scale: Vec3) -> f32 {
    let axes = scale.to_array();
    let mut stretch = 0.0_f32;
    for axis in 0..3_usize {
        let (Some(direction), Some(factor)) = (normal.get(axis), axes.get(axis)) else {
            continue;
        };
        let scaled = direction * factor;
        stretch = scaled.mul_add(scaled, stretch);
    }
    stretch.sqrt()
}

/// How far to lift, in metres: [`LIFT_FRACTION`] of the bounding box's diagonal
/// **as drawn** (the local extent stretched by `scale`), clamped to
/// [`LIFT_MIN`]..=[`LIFT_MAX`].
///
/// Shared with [`crate::selection_silhouette`]: a silhouette ribbon's inner edge
/// lies on the surface it outlines and z-fights it for exactly the same reason a
/// wireframe's lines do, so it starts at the same lift.
pub(crate) fn lift_distance(positions: &[[f32; 3]], scale: Vec3) -> f32 {
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
    let stretch = scale.to_array();
    let mut diagonal = 0.0_f32;
    for axis in 0..3_usize {
        let (Some(low), Some(high), Some(factor)) =
            (min.get(axis), max.get(axis), stretch.get(axis))
        else {
            continue;
        };
        let span = (high - low) * factor.abs();
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
    use bevy::math::Vec3;
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
        let wireframe =
            wireframe_mesh(&quad(), Vec3::ONE).expect("a triangle list yields a wireframe");
        assert_eq!(wireframe.primitive_topology(), PrimitiveTopology::LineList);
        let indices = wireframe.indices().expect("the wireframe is indexed");
        assert_eq!(indices.len(), 10, "five unique edges, two indices each");
    }

    /// The overlay keeps the skin attributes, because that is what makes it skin
    /// with the face it outlines — and dropping them is exactly the mismatch that
    /// quits the viewer.
    #[test]
    fn skin_attributes_survive() {
        let wireframe =
            wireframe_mesh(&quad(), Vec3::ONE).expect("a triangle list yields a wireframe");
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
        let wireframe =
            wireframe_mesh(&quad(), Vec3::ONE).expect("a triangle list yields a wireframe");
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

    /// The lift is a **world** distance, so the entity scale the overlay is drawn
    /// under is folded in. A mesh asset's geometry is normalized and its object's
    /// Second Life size lives on the geometry holder: at ten metres a side, the
    /// quad's drawn diagonal is `10√2`, whose `LIFT_FRACTION` clears
    /// [`LIFT_MAX`] — so the lift is the ceiling, `LIFT_MAX / 10` in the mesh's
    /// own units. Passing the local extent instead (what the rigged-only version
    /// did) would have lifted `√2 × LIFT_FRACTION` locally, ten centimetres out.
    #[test]
    fn the_lift_is_a_world_distance() {
        let scale = Vec3::splat(10.0);
        let wireframe = wireframe_mesh(&quad(), scale).expect("a triangle list yields a wireframe");
        let Some(VertexAttributeValues::Float32x3(positions)) =
            wireframe.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the wireframe carries positions");
        };
        assert!(
            2.0_f32.sqrt() * 10.0 * LIFT_FRACTION > LIFT_MAX,
            "the fixture is meant to exercise the ceiling"
        );
        let expected = LIFT_MAX / 10.0;
        for position in positions {
            let z = position
                .get(2)
                .copied()
                .expect("a three-component position");
            assert!(
                (z - expected).abs() < 1e-6,
                "the ceiling is {LIFT_MAX} metres, {expected} in mesh units, got {z}"
            );
        }
    }

    /// A non-uniform scale is handled per vertex rather than by one factor for the
    /// mesh: the quad's `+Z` normals ride the stretched axis, so the local lift is
    /// divided by that axis alone — the flattened `x`/`y` extent still shrinks the
    /// diagonal the fraction is taken of, but does not stretch the offset.
    #[test]
    fn a_squashed_axis_only_divides_the_normal_it_lies_along() {
        let scale = Vec3::new(1.0, 1.0, 4.0);
        let wireframe = wireframe_mesh(&quad(), scale).expect("a triangle list yields a wireframe");
        let Some(VertexAttributeValues::Float32x3(positions)) =
            wireframe.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the wireframe carries positions");
        };
        // The quad is flat in `z`, so its drawn diagonal is the unscaled `√2` and
        // the world lift is the same as the unit-scale case — but the normals lie
        // along the axis stretched fourfold, so the local offset is a quarter of it.
        let expected = 2.0_f32.sqrt() * LIFT_FRACTION / 4.0;
        for position in positions {
            let z = position
                .get(2)
                .copied()
                .expect("a three-component position");
            assert!(
                (z - expected).abs() < 1e-6,
                "a fourfold `z` divides the local lift by four: expected {expected}, got {z}"
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
        assert!(wireframe_mesh(&lines, Vec3::ONE).is_none());

        let mut unindexed = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        unindexed.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0_f32, 0.0, 0.0]; 3]);
        assert!(wireframe_mesh(&unindexed, Vec3::ONE).is_none());
    }
}
