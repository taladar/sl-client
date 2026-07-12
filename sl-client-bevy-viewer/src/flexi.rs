//! Flexible prims (Phase 32): fold a prim's `LLFlexibleObjectData` extra-param
//! block into the scene mirror.
//!
//! **Ingest (P32.1).** A "flexi" prim carries a flexible-object extra-param
//! ([`FlexibleData`]) marking its path as a soft chain that bends under a
//! simulated spring / gravity / wind force (set by the build tool's *Features →
//! Flexible Path* or by `llSetPrimitiveParams`). sl-proto already decodes that
//! block into a [`FlexibleData`] on `Object::extra.flexible` (a port of
//! `LLFlexibleObjectData::unpack` — the four packed tension / drag / gravity /
//! wind bytes, the two simulate-LOD "softness" bits stashed in their high bits,
//! and the trailing user-force vector); [`flexi_from_object`] lifts a present
//! block onto an [`ObjectFlexi`] component that [`apply_object`] attaches to (or
//! clears from) each object entity as its updates arrive — ready for the P32.2
//! CPU chain simulation that will deform / re-tessellate the flexi geometry.
//!
//! The reference viewer's `LLVOVolume::isFlexible` treats a prim as flexi
//! **exactly when** it carries this block (`getFlexibleObjectData()` non-null) —
//! there is no null / sentinel form the way particles carry a zero-CRC "stop"
//! system, so the ingest is a straight `Option` lift: present → attach, absent →
//! remove. A prim toggled flexi on or off in-world flips the extra-param block
//! present / absent, so refreshing the component on every update (the way
//! [`apply_light`](crate::lights) / [`apply_particles`](crate::particles) do)
//! tracks that toggle.
//!
//! Flexi is mutually exclusive with server physics — the reference forces a flexi
//! prim `FLAGS_PHANTOM` on and `FLAGS_USE_PHYSICS` off (`setIsFlexible`), so a
//! flexi prim never also carries the P31.2 physics-body marker. The deformation
//! is purely client-side (the simulator sends no per-frame flexi state), which is
//! why the whole feature is a viewer-side simulation built on the block ingested
//! here.
//!
//! The component rides the **object entity** (which carries the prim's world
//! position / rotation), so P32.2's chain simulation will ride the prim's
//! transform the way the reference viewer's `LLVolumeImplFlexible` anchors its
//! chain at the prim's root.
//!
//! **Simulate (P32.2).** [`simulate_flexi`] runs the CPU chain solver each frame:
//! for every flexi prim it reads the prim's live world pose from its
//! `GlobalTransform`, steps the [`FlexiChain`] (`sl_prim`'s faithful port of
//! Firestorm's `LLVolumeImplFlexible::doFlexibleUpdate`), reads the deformed
//! extrusion path back out, re-sweeps the prim's profile along it
//! ([`tessellate_with_path`]), and overwrites each face mesh's positions / normals
//! in place. The chain solver lives in `sl_prim` (pure, unit-tested); this module
//! owns the ECS glue — the persistent [`FlexiSimState`], the per-frame step, and
//! the mesh rewrite.
//!
//! The whole deformation is client-side spring / gravity / tension physics, not
//! rigid-body dynamics, so — unlike the P31 physical prims — it is **not** built on
//! `avian3d`: the reference is a bespoke chain solver (a distance-constrained,
//! angle-clamped node chain) that avian's rigid bodies do not model, so a faithful
//! port of that solver is the natural fit the roadmap's "where practical"
//! anticipates.
//!
//! Two documented simplifications ride on `sl_prim`'s solver (no wind field, no
//! screen-area LOD throttling) plus one here: the face **UVs** are set once at
//! build and not re-projected as the prim bends, so a planar-texgen face's
//! projection is frozen at the rest pose (ordinary per-face texgen UVs are
//! parametric and stay correct under any bend).
//!
//! [`apply_object`]: crate::objects
//! [`FlexiChain`]: sl_client_bevy::FlexiChain
//! [`tessellate_with_path`]: sl_client_bevy::tessellate_with_path

use crate::coords::bevy_to_sl_vec;
use bevy::prelude::*;
use sl_client_bevy::{
    FlexiAttributes, FlexiChain, FlexibleData, Object, PrimLod, PrimShapeFloat,
    tessellate_with_path,
};

/// The level of detail the flexi profile ring is tessellated at (P32.2). The
/// profile point count must stay constant between the initial build and the
/// per-frame deform (the mesh is rewritten in place), so it is fixed rather than
/// pixel-area managed; flexi prims are thin and few, so a smooth profile is cheap.
pub(crate) const FLEXI_LOD: PrimLod = PrimLod::High;

/// A component marking an object entity as a **flexible ("flexi") prim**, carrying
/// the decoded `LLFlexibleObjectData` parameters in Second Life semantics — ready
/// for P32.2 to drive a CPU chain simulation and deform the prim's path.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// [`apply_object`](crate::objects) as its updates arrive. See
/// [`flexi_from_object`] for the present-vs-absent lift.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct ObjectFlexi {
    /// The decoded flexible-object parameters: the simulate-LOD softness (0–3),
    /// path tension (stiffness), air friction (damping), gravity on the tip, wind
    /// sensitivity, and the constant user force pushing the path.
    pub(crate) data: FlexibleData,
    /// The prim's Second Life metre scale, refreshed every update so a **resized**
    /// flexi prim's chain length and profile size stay correct (P32.2). The chain
    /// bakes this into its metre geometry, so — unlike a rigid prim — the scale is
    /// carried here rather than on an (identity) geometry holder.
    pub(crate) scale: [f32; 3],
}

/// Lift an object's flexible-object block onto an [`ObjectFlexi`], or `None` when
/// the object is not (or is no longer) a flexi prim.
///
/// Mirrors the reference viewer's `LLVOVolume::isFlexible`: a prim is flexi
/// exactly when it carries a flexible-object extra-param block
/// (`getFlexibleObjectData()`), so this is a straight `Option` lift with no
/// sentinel to reject (unlike the particle system's zero-CRC "null" form).
pub(crate) fn flexi_from_object(object: &Object) -> Option<ObjectFlexi> {
    object.extra.flexible.clone().map(|data| ObjectFlexi {
        data,
        scale: [object.scale.x, object.scale.y, object.scale.z],
    })
}

/// Reconcile an object entity's [`ObjectFlexi`] component (P32.1) with its current
/// flexible-object block: insert / refresh it when the prim is flexi, remove it
/// when the prim was made rigid in-world (the block dropped) or never was flexi.
/// Called on both the spawn and update paths so a prim toggled flexi on or off
/// between updates is tracked, the way [`apply_light`](crate::lights) /
/// [`apply_particles`](crate::particles) are.
pub(crate) fn apply_flexi(entity: Entity, flexi: Option<ObjectFlexi>, commands: &mut Commands) {
    match flexi {
        Some(flexi) => {
            let data = &flexi.data;
            debug!(
                "object flexi prim: softness={} tension={:.2} air_friction={:.2} \
                 gravity={:.2} wind={:.2} user_force=({:.2},{:.2},{:.2})",
                data.softness,
                data.tension,
                data.air_friction,
                data.gravity,
                data.wind_sensitivity,
                data.user_force.x,
                data.user_force.y,
                data.user_force.z,
            );
            commands.entity(entity).insert(flexi);
        }
        None => {
            commands.entity(entity).remove::<ObjectFlexi>();
        }
    }
}

/// The persistent per-prim state driving the flexi chain simulation (P32.2),
/// attached to a flexi prim's object entity alongside its [`ObjectFlexi`] block.
///
/// Holds the [`FlexiChain`] (the solver's node state, carried across frames so the
/// chain has inertia), the prim's dequantized shape (to re-sweep the profile along
/// the deformed path), the geometry-holder entity (read each frame for the prim's
/// live metre scale, so a resized flexi prim stays correct), the softness the
/// chain was built at (to skip a frame if a rebuild for a changed softness is
/// pending), and the prim's face entities (whose meshes are rewritten in place).
///
/// Created / refreshed by [`apply_object`](crate::objects) on the spawn and shape-
/// rebuild paths, and removed when a prim is toggled rigid. [`simulate_flexi`]
/// advances it every frame.
#[derive(Component)]
pub(crate) struct FlexiSimState {
    /// The chain solver's persistent node state.
    pub(crate) chain: FlexiChain,
    /// The prim's dequantized shape, re-swept along the deformed path each frame.
    pub(crate) shape: PrimShapeFloat,
    /// The softness the chain was built at; a live change needs a fresh chain (the
    /// node count changes), so a mismatch skips this frame until the shape rebuild
    /// re-creates the state.
    pub(crate) softness: u8,
    /// The prim's face entities (one per non-empty tessellated face, in order),
    /// whose position / normal attributes are overwritten each frame.
    pub(crate) face_entities: Vec<Entity>,
}

/// Map a decoded [`FlexibleData`] block onto the pure solver's [`FlexiAttributes`]
/// (the same fields, with the user force flattened to a plain array).
pub(crate) const fn flexi_attributes(data: &FlexibleData) -> FlexiAttributes {
    FlexiAttributes {
        softness: data.softness,
        tension: data.tension,
        air_friction: data.air_friction,
        gravity: data.gravity,
        wind_sensitivity: data.wind_sensitivity,
        user_force: [data.user_force.x, data.user_force.y, data.user_force.z],
    }
}

/// The prim's world pose in Second Life region-local space (Z-up metres), read
/// from its Bevy `GlobalTransform` — the anchor pose the chain solver needs.
///
/// Reads through the hierarchy uniformly (root prim, linkset child, or worn
/// attachment) since the object entities carry no scale, so their global transform
/// is a rigid rotate + translate. Inverting the single Second Life → Bevy basis
/// change (a `-90°` turn about X, [`sl_to_bevy_rotation`](crate::coords)) recovers
/// the Second Life rotation; a single quaternion's `(x, y, z, w)` components denote
/// the same rotation in Bevy's `glam` (column-vector) convention and `sl_prim`'s
/// row-vector one, so they carry across verbatim (only *composition* order differs,
/// which the solver keeps internally consistent). The translation inverts the
/// position basis change directly.
fn sl_world_pose(global: &GlobalTransform) -> ([f32; 3], [f32; 4]) {
    let (_scale, rotation, translation) = global.to_scale_rotation_translation();
    let basis_inverse = Quat::from_rotation_x(core::f32::consts::FRAC_PI_2);
    let sl_rotation = basis_inverse.mul_quat(rotation);
    let sl_position = bevy_to_sl_vec(translation);
    (
        [sl_position.x, sl_position.y, sl_position.z],
        [sl_rotation.x, sl_rotation.y, sl_rotation.z, sl_rotation.w],
    )
}

/// Advance every flexi prim's chain one frame and re-tessellate its geometry
/// (P32.2) — the flexi counterpart of [`drive_particles`](crate::particles).
///
/// For each prim carrying a [`FlexiSimState`]: read its live world pose (anchor)
/// and metre scale, step the chain by the frame's `dt`, read the deformed path out,
/// re-sweep the profile along it, and overwrite each face mesh's positions /
/// normals in place (the face count and vertex layout are stable, so the meshes are
/// mutated rather than respawned). A prim whose softness changed since the chain was
/// built is skipped for the frame — the shape-fingerprint rebuild (which re-creates
/// the state at the new node count) has already run this frame in `update_objects`.
pub(crate) fn simulate_flexi(
    time: Res<Time>,
    mut sims: Query<(&ObjectFlexi, &mut FlexiSimState, &GlobalTransform)>,
    face_meshes: Query<&Mesh3d>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (flexi, mut sim, global) in &mut sims {
        // A pending softness change: the chain has the old node count until the
        // shape rebuild re-creates this state, so leave the geometry as-is.
        if flexi.data.softness != sim.softness {
            continue;
        }
        let attributes = flexi_attributes(&flexi.data);
        // The prim's live metre scale (refreshed on the component each update), so a
        // resize is reflected in the chain length and the baked metre geometry.
        let scale = flexi.scale;
        let (base_position, base_rotation) = sl_world_pose(global);

        sim.chain
            .step(&attributes, scale, base_position, base_rotation, dt);
        let path = sim.chain.path(base_position, base_rotation, scale);
        let prim = tessellate_with_path(&sim.shape, FLEXI_LOD, &path);

        // Rewrite each face mesh in place. The non-empty faces are produced in the
        // same order the initial build spawned `face_entities`, so they zip up.
        let mut faces = prim.faces.iter().filter(|face| !face.is_empty());
        for &face_entity in &sim.face_entities {
            let Some(face) = faces.next() else {
                break;
            };
            let Ok(Mesh3d(handle)) = face_meshes.get(face_entity) else {
                continue;
            };
            let Some(mut mesh) = meshes.get_mut(handle) else {
                continue;
            };
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, face.positions.clone());
            if !face.normals.is_empty() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, face.normals.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectFlexi, flexi_from_object};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{FlexibleData, Object, Vector};

    /// A minimal plain prim object with no extra params — the fixture the flexi
    /// tests decorate.
    fn bare_object() -> Object {
        use sl_client_bevy::{
            CircuitId, ObjectMotion, RegionHandle, RegionLocalObjectId, Rotation, Uuid,
        };
        // A fresh zero vector per use (`Vector` derives neither `Copy` nor
        // `Default`).
        const fn zero() -> Vector {
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        }
        Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(1),
            circuit: CircuitId::new(1),
            full_id: Uuid::from_u128(1).into(),
            parent_id: RegionLocalObjectId(0),
            pcode: 9,
            state: 0,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            motion: ObjectMotion {
                position: zero(),
                velocity: zero(),
                acceleration: zero(),
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero(),
                collision_plane: None,
            },
            owner_id: Uuid::from_u128(0),
            sound: Uuid::from_u128(0),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: sl_client_bevy::PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: sl_client_bevy::ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero(),
            joint_axis_or_anchor: zero(),
        }
    }

    /// A representative flexi block (mid-range softness / tension / gravity, a
    /// small steady user force).
    fn flexi_data() -> FlexibleData {
        FlexibleData {
            softness: 2,
            tension: 1.0,
            air_friction: 2.0,
            gravity: 0.3,
            wind_sensitivity: 0.0,
            user_force: Vector {
                x: 0.0,
                y: 0.0,
                z: -0.5,
            },
        }
    }

    /// An object with no flexible-object block is not a flexi prim.
    #[test]
    fn no_flexible_block_is_none() {
        assert_eq!(flexi_from_object(&bare_object()), None);
    }

    /// A prim carrying a flexible-object block lifts into a component holding it
    /// and the prim's scale (for the P32.2 metre-baked chain geometry).
    #[test]
    fn flexi_block_becomes_a_component() {
        let mut object = bare_object();
        let data = flexi_data();
        object.extra.flexible = Some(data.clone());
        assert_eq!(
            flexi_from_object(&object),
            Some(ObjectFlexi {
                data,
                scale: [1.0, 1.0, 1.0],
            })
        );
    }
}
