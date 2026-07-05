//! Integration tests for the `sl-avatar` public API (P12.5).
//!
//! The per-module unit tests in `skeleton` / `basemesh` / `params` exercise
//! their internals; these tests come at the crate from the *outside*, using
//! only the re-exported public surface (`sl_avatar::*`) an actual consumer
//! sees, and assert the structural invariants the road map calls out:
//! skeleton hierarchy + attachment / HUD point maps, `.llm` non-degenerate
//! counts + weight normalization, and param-table lookups + byte→value
//! dequantization. They share the same committed fixtures as the unit tests.

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_avatar::{
        AttachmentPoint, AttachmentPoints, BaseMesh, LodMesh, MorphWeights, ParamGroup, Skeleton,
        VisualParams,
    };

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The minimal committed skeleton fixture: 4 bones, 2 collision volumes.
    const MINI_SKELETON: &str = include_str!("fixtures/mini_skeleton.xml");
    /// The minimal committed attachment fixture: chest, skull, one HUD point.
    const MINI_LAD: &str = include_str!("fixtures/mini_lad.xml");
    /// The minimal committed full base-mesh fixture (4 verts / 2 faces / 2 joints).
    const MINI_BASEMESH: &[u8] = include_bytes!("fixtures/mini_basemesh.llm");
    /// The minimal committed reduced-LOD fixture (header + one face).
    const MINI_LOD: &[u8] = include_bytes!("fixtures/mini_basemesh_lod.llm");
    /// The minimal committed visual-param fixture (one param per effect type).
    const MINI_PARAMS: &str = include_str!("fixtures/mini_params.xml");

    /// Compare two floats within a tolerance (keeps assertions off `float_cmp`).
    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
    }

    #[test]
    fn skeleton_hierarchy_invariants_hold() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;

        // Non-degenerate, and the declared header counts match what we decoded.
        assert!(!skeleton.is_empty());
        assert_eq!(skeleton.declared_bone_count(), Some(skeleton.len()));
        assert_eq!(
            skeleton.declared_collision_volume_count(),
            Some(skeleton.collision_volume_count()),
        );

        // Exactly one root, and it has no parent.
        let roots = skeleton.roots();
        assert_eq!(roots.len(), 1);
        let root_index = roots.first().copied().ok_or("a root")?;
        let root = skeleton.joint(root_index).ok_or("root joint")?;
        assert_eq!(root.parent, None);

        // Every joint's parent precedes it (topological order), and each joint is
        // listed among its parent's children exactly once — a coherent tree.
        for (index, joint) in skeleton.joints().iter().enumerate() {
            match joint.parent {
                None => assert!(roots.contains(&index), "orphan joint {index} is a root"),
                Some(parent) => {
                    assert!(parent < index, "parent {parent} must precede child {index}");
                    let parent_joint = skeleton.joint(parent).ok_or("parent joint")?;
                    let listed = parent_joint
                        .children
                        .iter()
                        .filter(|&&child| child == index)
                        .count();
                    assert_eq!(listed, 1, "child {index} listed once under its parent");
                }
            }
        }
        Ok(())
    }

    #[test]
    fn skeleton_name_and_alias_lookups_round_trip() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;

        // Every canonical name and every alias resolves back to its own joint.
        for (index, joint) in skeleton.joints().iter().enumerate() {
            assert_eq!(skeleton.find(&joint.name), Some(index));
            let by_name = skeleton.joint_by_name(&joint.name).ok_or("joint by name")?;
            assert_eq!(by_name.name, joint.name);
            for alias in &joint.aliases {
                assert_eq!(skeleton.find(alias), Some(index), "alias {alias} resolves");
            }
        }
        // An unknown name resolves to nothing.
        assert_eq!(skeleton.find("mNotAJoint"), None);
        Ok(())
    }

    #[test]
    fn attachment_and_hud_maps_are_consistent() -> Result<(), TestError> {
        let points = AttachmentPoints::from_xml(MINI_LAD)?;

        // The join between the tabular map and the per-point classification agrees,
        // and each is consistent with the wire enum's own HUD flag.
        let hud = points.hud_points();
        for (point, joint) in points.joint_map() {
            assert_eq!(points.joint_for(point), Some(joint));
            assert_eq!(points.is_hud(point), hud.contains(&point));
            assert_eq!(points.is_hud(point), point.is_hud());
        }
        // Every HUD point is a member of the full set.
        let all: Vec<AttachmentPoint> = points.all().iter().map(|def| def.point).collect();
        for point in &hud {
            assert!(all.contains(point), "HUD point {point:?} is a known point");
        }
        Ok(())
    }

    #[test]
    fn attachment_joints_resolve_when_present_in_skeleton() -> Result<(), TestError> {
        // The two `character/` assets are decoded independently, so an attachment
        // point may name a joint the (minimal) skeleton fixture omits. Where the
        // named joint *is* present, the cross-asset reference must resolve — the
        // real link the viewer relies on to hang an attachment on a bone.
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        let points = AttachmentPoints::from_xml(MINI_LAD)?;

        let mut resolved = 0_usize;
        for (_point, joint) in points.joint_map() {
            if let Some(index) = skeleton.find(joint) {
                let found = skeleton.joint(index).ok_or("joint at index")?;
                assert!(
                    found.name == joint || found.aliases.iter().any(|alias| alias == joint),
                    "attachment joint {joint} matches the resolved bone",
                );
                resolved += 1;
            }
        }
        // mChest is shared by both fixtures, so at least one link resolves.
        assert!(
            resolved >= 1,
            "at least one attachment joint is in the skeleton"
        );
        Ok(())
    }

    #[test]
    fn base_mesh_counts_are_non_degenerate() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;

        let verts = mesh.vertex_count();
        assert!(verts > 0, "a base part has vertices");
        assert!(!mesh.faces().is_empty(), "a base part has faces");

        // Every per-vertex stream is exactly one entry per vertex.
        assert_eq!(mesh.positions().len(), verts);
        assert_eq!(mesh.normals().len(), verts);
        assert_eq!(mesh.binormals().len(), verts);
        assert_eq!(mesh.tex_coords().len(), verts);
        // Detail UVs are all-or-nothing.
        assert!(mesh.detail_tex_coords().is_empty() || mesh.detail_tex_coords().len() == verts);

        // Every triangle indexes real vertices.
        for face in mesh.faces() {
            for &vertex in face {
                assert!(usize::from(vertex) < verts, "face vertex {vertex} in range");
            }
        }

        // Morph deltas and the shared-vertex remap table stay in range.
        for morph in mesh.morphs() {
            for delta in &morph.deltas {
                assert!(delta.vertex_index < verts, "morph delta vertex in range");
            }
        }
        for shared in mesh.shared_verts() {
            assert!(shared.source < verts, "shared source in range");
            assert!(shared.destination < verts, "shared destination in range");
        }
        Ok(())
    }

    #[test]
    fn base_mesh_skin_weights_are_normalized() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        assert!(mesh.has_weights(), "the fixture is a weighted part");

        let joints = mesh.joint_names().len();
        assert!(joints > 0, "a weighted part names its skin joints");
        // One weight per vertex.
        assert_eq!(mesh.weights().len(), mesh.vertex_count());

        // The single on-disk weight float splits into an in-range joint index and a
        // fractional blend in [0, 1) toward the next joint. The last joint clamps
        // (blend 0) rather than blending past the table.
        for weight in mesh.weights() {
            assert!(
                weight.joint < joints,
                "weight joint {} in range",
                weight.joint
            );
            assert!(
                weight.blend >= 0.0 && weight.blend < 1.0,
                "blend {} normalized to [0, 1)",
                weight.blend,
            );
            if weight.joint == joints - 1 {
                assert!(
                    approx(weight.blend, 0.0),
                    "last joint does not blend past the table"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn reduced_lod_indexes_within_its_vertex_count() -> Result<(), TestError> {
        let lod = LodMesh::from_bytes(MINI_LOD)?;
        assert!(!lod.faces().is_empty(), "a LOD has faces");

        // `vertex_count` is one past the largest referenced index, so every face
        // index is strictly below it and the maximum index is exactly one less.
        let count = lod.vertex_count();
        let mut max_index = 0_u16;
        for face in lod.faces() {
            for &vertex in face {
                assert!(
                    usize::from(vertex) < count,
                    "LOD face vertex {vertex} in range"
                );
                max_index = max_index.max(vertex);
            }
        }
        assert_eq!(
            usize::from(max_index) + 1,
            count,
            "vertex_count is max index + 1"
        );
        Ok(())
    }

    #[test]
    fn param_table_is_id_sorted_and_lookups_round_trip() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        assert!(!params.is_empty());

        // Ascending id order (the reference viewer's `std::map` key order), and
        // every param resolves by its own id.
        let mut previous: Option<i32> = None;
        for param in params.all() {
            if let Some(prev) = previous {
                assert!(
                    param.id > prev,
                    "ids strictly ascending: {prev} then {}",
                    param.id
                );
            }
            previous = Some(param.id);
            let looked_up = params.get(param.id).ok_or("param resolves by id")?;
            assert_eq!(looked_up.id, param.id);
        }

        // The transmitted subset is exactly the params in a wire-carrying group,
        // still in id order, and its length matches the reported count.
        let transmitted = params.transmitted();
        assert_eq!(transmitted.len(), params.transmitted_count());
        for param in &transmitted {
            assert!(
                matches!(
                    param.group,
                    ParamGroup::Tweakable | ParamGroup::TransmitNotTweakable
                ),
                "transmitted param {} is in a wire group",
                param.id,
            );
        }
        // Non-transmitted params are absent from the wire subset.
        let non_transmitted = params
            .all()
            .iter()
            .filter(|param| !param.is_transmitted())
            .count();
        assert_eq!(transmitted.len() + non_transmitted, params.len());
        Ok(())
    }

    #[test]
    fn appearance_bytes_dequantize_and_match_per_param() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        let transmitted = params.transmitted();

        // Drive a full wire vector: byte 255 for even slots, byte 0 for odd, so we
        // hit both ends of several dequantization ramps in one pass.
        let bytes: Vec<u8> = transmitted
            .iter()
            .enumerate()
            .map(|(slot, _)| if slot % 2 == 0 { 255 } else { 0 })
            .collect();
        let values = params.map_appearance(&bytes);
        assert_eq!(values.len(), transmitted.len());

        // The vector API and the per-param `weight_from_byte` must agree slot for
        // slot, and the byte→value ramp is bounded by the param's own min/max.
        for (slot, param) in transmitted.iter().enumerate() {
            let byte = if slot % 2 == 0 { 255 } else { 0 };
            let expected = param.weight_from_byte(byte);
            let actual = values.weight(param.id).ok_or("mapped weight present")?;
            assert!(
                approx(actual, expected),
                "slot {slot} matches weight_from_byte"
            );
            let (lo, hi) = (param.min.min(param.max), param.min.max(param.max));
            assert!(
                actual >= lo - 1.0e-4 && actual <= hi + 1.0e-4,
                "weight within bounds"
            );
        }
        Ok(())
    }

    #[test]
    fn short_and_empty_appearance_vectors_fall_back_to_defaults() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        let transmitted = params.transmitted();

        // An empty vector: every transmitted param sits at its default, with no raw
        // byte recorded.
        let empty = params.map_appearance(&[]);
        assert_eq!(empty.len(), transmitted.len());
        for param in &transmitted {
            assert!(
                empty
                    .weight(param.id)
                    .is_some_and(|w| approx(w, param.default)),
                "empty vector leaves {} at its default",
                param.id,
            );
        }
        assert!(empty.values().iter().all(|value| value.byte.is_none()));

        // A single supplied byte: the first param takes it, the rest default.
        let one = params.map_appearance(&[255]);
        assert_eq!(one.len(), transmitted.len());
        let supplied = one
            .values()
            .iter()
            .filter(|value| value.byte.is_some())
            .count();
        assert_eq!(supplied, 1, "exactly one slot carries a raw byte");
        Ok(())
    }

    #[test]
    fn morph_blend_invariants_hold() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        let params = VisualParams::from_xml(MINI_PARAMS)?;

        // Resolving an appearance vector against the table yields morph weights
        // only for morph-effect params (the fixture's single morph param), never
        // for the skeletal / colour / alpha ones — and a full-weight vector still
        // does not drive the base mesh's own morph, whose name differs.
        let resolved = MorphWeights::from_appearance(&params, &[255, 255, 255, 255, 255]);
        let morph = mesh.morphs().first().ok_or("a morph target")?;
        assert!(resolved.weight(&morph.name).abs() < f32::EPSILON);

        // Applying any weights preserves the vertex count exactly (a morph adds or
        // drops no vertices) and re-normalizes every normal to unit length (or a
        // preserved degenerate zero).
        let morphed = resolved.apply(&mesh);
        assert_eq!(morphed.positions().len(), mesh.vertex_count());
        assert_eq!(morphed.normals().len(), mesh.vertex_count());
        for normal in morphed.normals() {
            let [x, y, z] = *normal;
            let length = (x * x + y * y + z * z).sqrt();
            assert!(
                (length - 1.0).abs() < 1.0e-3 || length < f32::EPSILON,
                "normal stays unit length",
            );
        }

        // No matching driven param → the blend is the identity on positions.
        assert_eq!(morphed.positions(), mesh.positions());
        Ok(())
    }
}
