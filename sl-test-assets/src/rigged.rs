//! **Rigged** mesh fixtures — the skinned half of [`crate::mesh`]: mesh assets
//! that carry a `skin` block and per-vertex joint influences, which is what a
//! mesh body, mesh clothing or an animesh object is made of.
//!
//! Real rigged content was considered and rejected as a fixture source. A
//! Library asset's in-world permissions are not a redistribution licence, a
//! mesh body is megabytes across four levels of detail, and — decisively — a
//! well-made body cannot support the assertion actually wanted here. A
//! two-bone cylinder whose weights ramp linearly along its axis has a
//! **closed-form** deformed position per vertex, so an oracle asserts exact
//! numbers rather than "looks about right".
//!
//! Beside the well-formed one are deliberately **pathological** rigs, because
//! what breaks skinning code is never average content: weights that do not sum
//! to one, a vertex with no influences at all, a vertex carrying the full four
//! (whose influence list therefore has no `0xFF` terminator), an influence
//! naming a joint the `skin` block never lists, Bento joint names with a
//! joint-position override, and levels of detail whose `Weights` streams
//! disagree. `sl_mesh::encode` polices the *format*, never the content, which
//! is exactly what makes these writable.

use sl_mesh::{MeshEncodeError, MeshLod, MeshModel, MeshSkin, Submesh, VertexWeights, encode_mesh};

/// A whole turn in radians, the angle the cylinder's segments divide.
const TURN: f32 = std::f32::consts::TAU;

/// The joints every two-bone fixture here binds to, in the `joint_names` order
/// the `skin` block carries them.
///
/// `mChest` is joint **1** deliberately: it is the joint
/// [`crate::anim::chest_twist_animation_asset`] rotates, so one cylinder is
/// both the static rigged fixture and the animesh one — the motion a fixture
/// grid already serves twists its upper half and leaves its lower half where
/// it was.
pub const RIG_JOINTS: [&str; 2] = ["mTorso", "mChest"];

/// The rigged cylinder's height in metres. One, so a vertex's height above the
/// cylinder's base *is* its position along the weight ramp.
pub const CYLINDER_HEIGHT: f32 = 1.0;

/// The rigged cylinder's radius in metres.
pub const CYLINDER_RADIUS: f32 = 0.25;

/// How many segments the cylinder is divided into around its axis. The seam
/// column is duplicated so the texture coordinate can run a full `0..1`, which
/// makes the vertex count `(segments + 1) * (rings + 1)`.
pub const CYLINDER_SEGMENTS: usize = 8;

/// How many rings the cylinder is divided into along its axis. Four rings is
/// five rows of vertices, so the ramp is sampled at 0, ¼, ½, ¾ and 1 —
/// including both ends, where the decoder's weight clamp bites.
pub const CYLINDER_RINGS: usize = 4;

/// The influences a cylinder vertex carries at height fraction `along`
/// (`0` at the base, `1` at the top): the lower joint's share ramps from one to
/// zero and the upper joint's from zero to one.
///
/// The blend is linear in `along`, so a vertex's deformed position is a linear
/// blend of the two joint transforms and has a closed form. These are the
/// values *written*: `sl-mesh`'s decoder clamps a weight into the reference's
/// `[0.001, 0.999]`, so a vertex at either end reads back as that bound rather
/// than as an exact `0` or `1`.
#[must_use]
pub fn cylinder_influences(along: f32) -> [(u8, f32); 2] {
    let along = along.clamp(0.0, 1.0);
    [(0, 1.0 - along), (1, along)]
}

/// A two-bone cylinder standing upright in the normalized mesh box: its axis
/// on `z`, spanning `[-0.5, 0.5]`, [`CYLINDER_RADIUS`] across, with outward
/// normals, a `0..1` texture coordinate around and along it, and the linear
/// weight ramp of [`cylinder_influences`].
///
/// The bind matrices are identity — the rest pose is the mesh as authored, so
/// a vertex that moves at rest moved because the skinning maths is wrong and
/// not because the bind pose said so.
#[must_use]
pub fn cylinder() -> (Submesh, MeshSkin) {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut weights = Vec::new();
    for ring in 0..=CYLINDER_RINGS {
        let along = fraction(ring, CYLINDER_RINGS);
        let height = CYLINDER_HEIGHT * 0.5;
        for column in 0..=CYLINDER_SEGMENTS {
            let around = fraction(column, CYLINDER_SEGMENTS);
            let (sin, cos) = (around * TURN).sin_cos();
            positions.push([
                cos * CYLINDER_RADIUS,
                sin * CYLINDER_RADIUS,
                along * CYLINDER_HEIGHT - height,
            ]);
            normals.push([cos, sin, 0.0]);
            uvs.push([around, along]);
            weights.push(VertexWeights {
                influences: cylinder_influences(along).to_vec(),
            });
        }
    }

    // Two triangles per quad, wound counter-clockwise seen from outside so the
    // outward normals above and the face winding agree.
    let stride = CYLINDER_SEGMENTS.saturating_add(1);
    let mut indices = Vec::new();
    for ring in 0..CYLINDER_RINGS {
        for column in 0..CYLINDER_SEGMENTS {
            let low = ring.saturating_mul(stride).saturating_add(column);
            let high = low.saturating_add(stride);
            let quad = [
                index(low),
                index(low.saturating_add(1)),
                index(high.saturating_add(1)),
                index(low),
                index(high.saturating_add(1)),
                index(high),
            ];
            indices.extend_from_slice(&quad);
        }
    }

    let submesh = Submesh {
        positions,
        normals,
        uvs,
        indices,
        weights: Some(weights),
        normalized_scale: [1.0, 1.0, 1.0],
        no_geometry: false,
    };
    (submesh, two_bone_skin())
}

/// [`cylinder`] as a whole mesh asset, the same geometry at every level
/// of detail — so however aggressively a viewer picks its level, the cylinder
/// it skins is the one the oracle predicts.
///
/// # Errors
///
/// Returns [`MeshEncodeError`] if the encoder refuses the model, which a
/// forty-five-vertex cylinder in one face cannot provoke.
pub fn cylinder_mesh_asset() -> Result<Vec<u8>, MeshEncodeError> {
    let (submesh, skin) = cylinder();
    encode_mesh(
        &MeshModel::default()
            .with_every_lod(vec![submesh])
            .with_skin(skin),
    )
}

/// The joints [`pathological_rig`]'s `skin` block names. Four, so its
/// four-influence vertex names four *real* joints and the no-terminator case
/// stays distinct from the dangling-joint one.
pub const PATHOLOGICAL_JOINTS: [&str; 4] = ["mTorso", "mChest", "mNeck", "mHead"];

/// The [`pathological_rig`] vertex whose influences sum to **less than one**.
/// Bevy's skinning shader does not renormalize, so an unnormalised rig blends
/// in a fraction of the zero matrix and drags the vertex toward the mesh
/// origin — the distortion `to_bevy_rigged_mesh` exists to fix.
pub const UNNORMALISED_VERTEX: usize = 0;

/// What [`UNNORMALISED_VERTEX`]'s influences sum to.
pub const UNNORMALISED_SUM: f32 = 0.6;

/// The [`pathological_rig`] vertex with **no influences at all**. The decoder
/// reports an empty list and the viewer's packer applies the reference's
/// fallback — full weight on joint 0 — so the two agree in effect; this vertex
/// pins that they keep agreeing.
pub const UNWEIGHTED_VERTEX: usize = 1;

/// The [`pathological_rig`] vertex carrying the full four influences, whose
/// stream therefore ends **without** a `0xFF` terminator.
pub const FOUR_INFLUENCE_VERTEX: usize = 2;

/// The [`pathological_rig`] vertex whose single influence names a joint index
/// the `skin` block does not have.
pub const DANGLING_JOINT_VERTEX: usize = 3;

/// The joint index [`DANGLING_JOINT_VERTEX`] names — past the four of
/// [`PATHOLOGICAL_JOINTS`], and well inside the `0..=254` the wire allows, so
/// it is the *skin* that lacks the joint rather than the format that refuses
/// it.
pub const DANGLING_JOINT_INDEX: u8 = 9;

/// A four-vertex quad whose every vertex is a different malformed rig: see
/// [`UNNORMALISED_VERTEX`], [`UNWEIGHTED_VERTEX`], [`FOUR_INFLUENCE_VERTEX`]
/// and [`DANGLING_JOINT_VERTEX`].
///
/// One face rather than four fixtures because the four cases are independent
/// per vertex, and a consumer that mishandles one of them shows it against the
/// three beside it.
#[must_use]
pub fn pathological_rig() -> (Submesh, MeshSkin) {
    let quarter = UNNORMALISED_SUM / 2.0;
    let weights = vec![
        VertexWeights {
            influences: vec![(0, quarter), (1, quarter)],
        },
        VertexWeights {
            influences: Vec::new(),
        },
        VertexWeights {
            influences: vec![(0, 0.25), (1, 0.25), (2, 0.25), (3, 0.25)],
        },
        VertexWeights {
            influences: vec![(DANGLING_JOINT_INDEX, 1.0)],
        },
    ];
    let mut submesh = quad();
    submesh.weights = Some(weights);
    (submesh, skin_named(&PATHOLOGICAL_JOINTS))
}

/// [`pathological_rig`] as a whole mesh asset.
///
/// # Errors
///
/// Returns [`MeshEncodeError`] if the encoder refuses the model. It does not:
/// every pathology here is *content*, and the encoder polices only the format.
pub fn pathological_rig_mesh_asset() -> Result<Vec<u8>, MeshEncodeError> {
    let (submesh, skin) = pathological_rig();
    encode_mesh(
        &MeshModel::default()
            .with_every_lod(vec![submesh])
            .with_skin(skin),
    )
}

/// The joints [`bento_override_rig`] names: one base joint and two **Bento**
/// bones, which a pre-Bento rig would not have.
pub const BENTO_JOINTS: [&str; 3] = ["mTorso", "mWingsRoot", "mTail1"];

/// How far up the `z` axis [`bento_override_rig`]'s joint-position override
/// moves its joints, in metres.
pub const BENTO_JOINT_LIFT: f32 = 0.25;

/// The pelvis `z` offset [`bento_override_rig`] carries.
pub const BENTO_PELVIS_OFFSET: f32 = 0.35;

/// A quad on a **Bento** rig that overrides its joint positions: the skin
/// carries an `alt_inverse_bind_matrix` per joint (each lifted by
/// [`BENTO_JOINT_LIFT`]), a [`BENTO_PELVIS_OFFSET`] and the
/// `lock_scale_if_joint_position` flag — the three optional `skin` fields a
/// rig with joint-position overrides sets and an ordinary one omits.
#[must_use]
pub fn bento_override_rig() -> (Submesh, MeshSkin) {
    let mut submesh = quad();
    submesh.weights = Some(vec![
        VertexWeights {
            influences: vec![(0, 1.0)],
        },
        VertexWeights {
            influences: vec![(0, 0.5), (1, 0.5)],
        },
        VertexWeights {
            influences: vec![(1, 0.5), (2, 0.5)],
        },
        VertexWeights {
            influences: vec![(2, 1.0)],
        },
    ]);
    let mut skin = skin_named(&BENTO_JOINTS);
    skin.alt_inverse_bind_matrix =
        vec![translation([0.0, 0.0, BENTO_JOINT_LIFT]); BENTO_JOINTS.len()];
    skin.pelvis_offset = Some(BENTO_PELVIS_OFFSET);
    skin.lock_scale_if_joint_position = true;
    (submesh, skin)
}

/// [`bento_override_rig`] as a whole mesh asset.
///
/// # Errors
///
/// Returns [`MeshEncodeError`] if the encoder refuses the model, which three
/// joints on one quad cannot provoke.
pub fn bento_override_rig_mesh_asset() -> Result<Vec<u8>, MeshEncodeError> {
    let (submesh, skin) = bento_override_rig();
    encode_mesh(
        &MeshModel::default()
            .with_every_lod(vec![submesh])
            .with_skin(skin),
    )
}

/// A cylinder whose levels of detail carry the **same geometry** but different
/// `Weights` streams: `high_lod` ramps along the axis, every coarser level
/// binds every vertex entirely to joint 0.
///
/// Weights live in the geometry block, not the `skin` block, so a real asset
/// *can* disagree with itself between levels — and a viewer that caches the
/// rig from whichever level it decoded first will show one level's deformation
/// on another level's geometry. Switching this fixture's level visibly
/// straightens the cylinder.
///
/// # Errors
///
/// Returns [`MeshEncodeError`] if the encoder refuses the model; the levels
/// are the same size, which the decimation rule allows.
pub fn lod_weight_mismatch_mesh_asset() -> Result<Vec<u8>, MeshEncodeError> {
    let (ramped, skin) = cylinder();
    let mut bound = ramped.clone();
    bound.weights = Some(vec![
        VertexWeights {
            influences: vec![(0, 1.0)],
        };
        ramped.positions.len()
    ]);
    let mut model = MeshModel::default()
        .with_lod(MeshLod::High, vec![ramped])
        .with_skin(skin);
    for lod in [MeshLod::Lowest, MeshLod::Low, MeshLod::Medium] {
        model = model.with_lod(lod, vec![bound.clone()]);
    }
    encode_mesh(&model)
}

/// The unit quad every non-cylinder fixture here rigs: four vertices in the
/// `xz` plane facing `-y`, two triangles, `0..1` texture coordinates.
fn quad() -> Submesh {
    Submesh {
        positions: vec![
            [-0.5, 0.0, -0.5],
            [0.5, 0.0, -0.5],
            [0.5, 0.0, 0.5],
            [-0.5, 0.0, 0.5],
        ],
        normals: vec![[0.0, -1.0, 0.0]; 4],
        uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        indices: vec![0, 1, 2, 0, 2, 3],
        weights: None,
        normalized_scale: [1.0, 1.0, 1.0],
        no_geometry: false,
    }
}

/// The two-joint skin of [`RIG_JOINTS`], all binds identity.
fn two_bone_skin() -> MeshSkin {
    skin_named(&RIG_JOINTS)
}

/// A skin naming `joints`, with an identity inverse bind per joint and an
/// identity bind shape.
fn skin_named(joints: &[&str]) -> MeshSkin {
    MeshSkin {
        joint_names: joints.iter().map(|name| (*name).to_owned()).collect(),
        inverse_bind_matrix: vec![identity(); joints.len()],
        bind_shape_matrix: identity(),
        alt_inverse_bind_matrix: Vec::new(),
        pelvis_offset: None,
        lock_scale_if_joint_position: false,
    }
}

/// The 4×4 identity matrix, row-major over 16 floats — the layout a `skin`
/// block's matrices use.
fn identity() -> [f32; 16] {
    let mut matrix = [0.0_f32; 16];
    for (position, slot) in matrix.iter_mut().enumerate() {
        if position.checked_rem(5) == Some(0) {
            *slot = 1.0;
        }
    }
    matrix
}

/// A pure translation as a row-major, **row-vector** 4×4 matrix: the offset
/// lives in the last row, which is where Second Life's matrices carry it.
fn translation(offset: [f32; 3]) -> [f32; 16] {
    let mut matrix = identity();
    for (slot, component) in matrix.iter_mut().skip(12).zip(offset) {
        *slot = component;
    }
    matrix
}

/// `step / of` as an `f32`, for a step count small enough to be exact.
fn fraction(step: usize, of: usize) -> f32 {
    let of = f32::from(u16::try_from(of).unwrap_or(1)).max(1.0);
    f32::from(u16::try_from(step).unwrap_or(0)) / of
}

/// A vertex ordinal as the `u32` a triangle list names it by.
fn index(vertex: usize) -> u32 {
    u32::try_from(vertex).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_mesh::{DecodedMesh, MeshLod, MeshSkin, Submesh};

    use super::{
        BENTO_JOINT_LIFT, BENTO_JOINTS, BENTO_PELVIS_OFFSET, CYLINDER_HEIGHT, CYLINDER_RADIUS,
        CYLINDER_RINGS, CYLINDER_SEGMENTS, DANGLING_JOINT_INDEX, DANGLING_JOINT_VERTEX,
        FOUR_INFLUENCE_VERTEX, PATHOLOGICAL_JOINTS, RIG_JOINTS, UNNORMALISED_SUM,
        UNNORMALISED_VERTEX, UNWEIGHTED_VERTEX, bento_override_rig_mesh_asset, cylinder_influences,
        cylinder_mesh_asset, lod_weight_mismatch_mesh_asset, pathological_rig_mesh_asset,
    };

    type TestError = Box<dyn core::error::Error>;

    /// The narrowest and widest weight `sl-mesh`'s decoder will report, the
    /// reference's own clamp. A fixture that writes an exact `0` or `1` reads
    /// back as one of these.
    const DECODED_WEIGHT_RANGE: (f32, f32) = (0.001, 0.999);

    /// How far a round trip may move a value: the `u16` quantization step over
    /// the widest domain any fixture here spans, with room for the `f32`
    /// arithmetic on either side.
    const TOLERANCE: f32 = 1e-3;

    /// Decodes one level of an encoded asset, plus its `skin` block.
    fn decode(asset: &[u8], lod: MeshLod) -> Result<(DecodedMesh, MeshSkin), TestError> {
        let (header, header_size) = sl_mesh::parse_header(asset).ok_or("no mesh header")?;
        let geometry = header.lod(lod).ok_or("no such level of detail")?;
        let (start, end) = geometry.range(header_size);
        let mesh = sl_mesh::decode_lod(asset.get(start..end).ok_or("lod out of range")?, lod)?;
        let block = header.skin.ok_or("no skin block")?;
        let (start, end) = block.range(header_size);
        let skin = sl_mesh::decode_skin(asset.get(start..end).ok_or("skin out of range")?)?;
        Ok((mesh, skin))
    }

    /// The only face of a decoded level.
    fn face(mesh: &DecodedMesh) -> Option<&Submesh> {
        mesh.submeshes.first()
    }

    /// The weight the decoder reports for a nominal `weight`, normalized
    /// against its partner so the pair sums to one — what a renderer's packer
    /// ends up with.
    fn decoded_share(weight: f32) -> f32 {
        let (low, high) = DECODED_WEIGHT_RANGE;
        let mine = weight.clamp(low, high);
        let theirs = (1.0 - weight).clamp(low, high);
        mine / (mine + theirs)
    }

    /// The cylinder decodes back into the cylinder it was written as, and each
    /// vertex's influences are the ramp its height asks for.
    #[test]
    fn the_cylinder_round_trips_with_its_ramp() -> Result<(), TestError> {
        let asset = cylinder_mesh_asset()?;
        for lod in MeshLod::ALL {
            let (mesh, skin) = decode(&asset, lod)?;
            assert_eq!(skin.joint_names, RIG_JOINTS.to_vec());
            assert_eq!(skin.inverse_bind_matrix.len(), RIG_JOINTS.len());
            assert!(skin.alt_inverse_bind_matrix.is_empty());
            assert_eq!(skin.pelvis_offset, None);

            let face = face(&mesh).ok_or("no face")?;
            let columns = CYLINDER_SEGMENTS.saturating_add(1);
            let rows = CYLINDER_RINGS.saturating_add(1);
            assert_eq!(face.positions.len(), columns.saturating_mul(rows));
            assert_eq!(
                mesh.triangle_count(),
                CYLINDER_SEGMENTS
                    .saturating_mul(CYLINDER_RINGS)
                    .saturating_mul(2)
            );

            let weights = face.weights.as_ref().ok_or("no weights")?;
            assert_eq!(weights.len(), face.positions.len());
            for (position, vertex) in face.positions.iter().zip(weights) {
                // Every vertex sits on the cylinder's surface.
                let [x, y, z] = *position;
                assert!(
                    (x.hypot(y) - CYLINDER_RADIUS).abs() <= TOLERANCE,
                    "vertex {position:?} is off the cylinder's surface"
                );
                let along = (z + CYLINDER_HEIGHT * 0.5) / CYLINDER_HEIGHT;
                let [(_, lower), (_, upper)] = cylinder_influences(along);
                let (low, high) = DECODED_WEIGHT_RANGE;
                let expected = [
                    (0_u8, lower.clamp(low, high)),
                    (1_u8, upper.clamp(low, high)),
                ];
                assert_eq!(vertex.influences.len(), expected.len());
                for (&(joint, weight), (wanted_joint, wanted)) in
                    vertex.influences.iter().zip(expected)
                {
                    assert_eq!(joint, wanted_joint);
                    assert!(
                        (weight - wanted).abs() <= TOLERANCE,
                        "vertex at {along} carries {weight} for joint {joint}, not {wanted}"
                    );
                }
            }
        }
        Ok(())
    }

    /// The point of a linear ramp on identity binds: with joint 0 left where it
    /// is and joint 1 translated by `shear`, every vertex's deformed position
    /// is its authored position plus its **upper** share of that translation.
    ///
    /// The expectation comes from the vertex's *height*, not from its decoded
    /// weights, so a wrong weight stream cannot satisfy it by agreeing with
    /// itself.
    #[test]
    fn the_cylinder_deforms_to_its_closed_form() -> Result<(), TestError> {
        /// How far joint 1 is translated along `x`. One metre, so the
        /// deformation is the same order as the mesh and a fractional error in
        /// a weight is a visible error in a position.
        const SHEAR: f32 = 1.0;

        let asset = cylinder_mesh_asset()?;
        let (mesh, _skin) = decode(&asset, MeshLod::High)?;
        let face = face(&mesh).ok_or("no face")?;
        let weights = face.weights.as_ref().ok_or("no weights")?;
        for (position, vertex) in face.positions.iter().zip(weights) {
            let [x, _, z] = *position;
            // Skin the vertex the way a renderer does — but the binds are
            // identity and joint 0 *is* the identity, so
            // `Sum(wj / total) * (Jj * v)` collapses to the authored position
            // plus joint 1's normalized share of its translation. Only `x`
            // can move, because the shear is along `x` alone.
            let total: f32 = vertex.influences.iter().map(|&(_, weight)| weight).sum();
            let upper: f32 = vertex
                .influences
                .iter()
                .filter(|&&(joint, _)| joint == 1)
                .map(|&(_, weight)| weight)
                .sum();
            let skinned_x = x + (upper / total) * SHEAR;

            // The expectation comes from the ramp evaluated at this vertex's
            // height, never from the weights just read back.
            let along = (z + CYLINDER_HEIGHT * 0.5) / CYLINDER_HEIGHT;
            let nominal = cylinder_influences(along)
                .get(1)
                .map_or(0.0, |&(_, weight)| weight);
            let expected_x = x + decoded_share(nominal) * SHEAR;
            assert!(
                (skinned_x - expected_x).abs() <= TOLERANCE,
                "vertex {position:?} skinned to x={skinned_x}, not the closed-form {expected_x}"
            );
        }
        Ok(())
    }

    /// Each pathological vertex survives the round trip as the malformed thing
    /// it was written as — the encoder polices the format, never the content.
    #[test]
    fn every_pathological_vertex_survives_the_round_trip() -> Result<(), TestError> {
        let asset = pathological_rig_mesh_asset()?;
        let (mesh, skin) = decode(&asset, MeshLod::High)?;
        assert_eq!(skin.joint_names, PATHOLOGICAL_JOINTS.to_vec());
        let face = face(&mesh).ok_or("no face")?;
        let weights = face.weights.as_ref().ok_or("no weights")?;
        assert_eq!(weights.len(), 4);

        let influences = |vertex: usize| -> Result<Vec<(u8, f32)>, TestError> {
            Ok(weights
                .get(vertex)
                .ok_or("missing vertex")?
                .influences
                .clone())
        };

        // The unnormalised vertex still sums to what it was written as.
        let unnormalised = influences(UNNORMALISED_VERTEX)?;
        assert_eq!(unnormalised.len(), 2);
        let sum: f32 = unnormalised.iter().map(|&(_, weight)| weight).sum();
        assert!(
            (sum - UNNORMALISED_SUM).abs() <= TOLERANCE,
            "the unnormalised vertex sums to {sum}, not {UNNORMALISED_SUM}"
        );

        // The unweighted vertex carries nothing at all.
        assert!(influences(UNWEIGHTED_VERTEX)?.is_empty());

        // The four-influence vertex keeps all four — its stream carries no
        // terminator, and the decoder stops on the count rather than a byte.
        assert_eq!(influences(FOUR_INFLUENCE_VERTEX)?.len(), 4);

        // The dangling influence names a joint the skin does not have. It is
        // the *skin* that lacks it: the index is inside what the wire allows.
        let dangling = influences(DANGLING_JOINT_VERTEX)?;
        assert_eq!(dangling.len(), 1);
        assert_eq!(
            dangling.first().map(|&(joint, _)| joint),
            Some(DANGLING_JOINT_INDEX)
        );
        assert!(usize::from(DANGLING_JOINT_INDEX) >= skin.joint_names.len());
        Ok(())
    }

    /// The Bento rig carries its joint-position override: an alternate bind
    /// per joint, the pelvis offset and the scale-lock flag.
    #[test]
    fn the_bento_rig_carries_its_joint_position_override() -> Result<(), TestError> {
        let asset = bento_override_rig_mesh_asset()?;
        let (_mesh, skin) = decode(&asset, MeshLod::High)?;
        assert_eq!(skin.joint_names, BENTO_JOINTS.to_vec());
        assert_eq!(skin.alt_inverse_bind_matrix.len(), BENTO_JOINTS.len());
        assert!(skin.lock_scale_if_joint_position);
        let offset = skin.pelvis_offset.ok_or("no pelvis offset")?;
        assert!((offset - BENTO_PELVIS_OFFSET).abs() <= TOLERANCE);
        // The alternate bind is the identity lifted along `z`: element 14 is a
        // row-vector matrix's `z` translation.
        let alt = skin
            .alt_inverse_bind_matrix
            .first()
            .ok_or("no alternate bind")?;
        assert_eq!(alt.get(14).copied(), Some(BENTO_JOINT_LIFT));
        assert_eq!(alt.first().copied(), Some(1.0));
        Ok(())
    }

    /// Weights live in the geometry block, so the levels of detail can — and
    /// here do — disagree about the rig while carrying the same geometry.
    #[test]
    fn the_levels_of_detail_carry_different_weight_streams() -> Result<(), TestError> {
        let asset = lod_weight_mismatch_mesh_asset()?;
        let (high, _skin) = decode(&asset, MeshLod::High)?;
        let (lowest, _skin) = decode(&asset, MeshLod::Lowest)?;
        let high_face = face(&high).ok_or("no high face")?;
        let lowest_face = face(&lowest).ok_or("no lowest face")?;
        assert_eq!(high_face.positions.len(), lowest_face.positions.len());

        let high_weights = high_face.weights.as_ref().ok_or("no high weights")?;
        let lowest_weights = lowest_face.weights.as_ref().ok_or("no lowest weights")?;
        // Every coarse vertex binds to joint 0 alone.
        for vertex in lowest_weights {
            assert_eq!(vertex.influences.len(), 1);
            assert_eq!(vertex.influences.first().map(|&(joint, _)| joint), Some(0));
        }
        // The fine level ramps, so its topmost vertices lean on joint 1.
        let top = high_weights.last().ok_or("no top vertex")?;
        assert_eq!(top.influences.len(), 2);
        let upper = top.influences.get(1).map_or(0.0, |&(_, weight)| weight);
        assert!(upper > 0.9, "the top vertex leans on joint 1, not {upper}");
        Ok(())
    }
}
