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
//! **Bounds and picking (`viewer-flexi-prim-picking`).** The in-place rewrite
//! goes through `Assets::get_mut`, which marks the mesh asset changed — and
//! Bevy's `calculate_bounds` refreshes a changed mesh's [`Aabb`] the same frame
//! (its `AssetChanged<Mesh3d>` branch). The flexi faces are therefore ordinary
//! `Aabb`-managed entities (no `NoFrustumCulling` opt-out): frustum culling
//! follows the bent geometry, and — because `MeshRayCast` reads the `Aabb`
//! non-optionally — the world ray-cast picks (left-click touch,
//! [`crate::object_menu`]'s right-click pie) hit a flexi exactly where it is
//! drawn. Before this, the opt-out left flexi faces with no `Aabb` at all,
//! making every flexi prim silently untouchable and un-menu-able.
//!
//! [`Aabb`]: bevy::camera::primitives::Aabb
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

/// The per-step chain movement (metres) below which a flexi prim's chain is
/// considered to have **settled** onto its rest pose (`viewer-perf-flexi-settle-lod`).
///
/// When [`FlexiChain::step`] reports the chain moved less than this in a frame, the
/// transient is over: [`simulate_flexi`] does one last geometry rewrite and then
/// **latches** the prim settled (recording the pose / attributes / scale it settled
/// at, its [`FlexiRest`]), after which it is frozen and costs nothing until one of
/// those inputs changes. A settled chain sits on a tiny residual limit cycle well
/// below this threshold (~0.1 mm/step), so it latches reliably; 0.3 mm is also the
/// worst-case geometry error frozen at the latch, far below a pixel at any normal
/// viewing distance.
///
/// [`FlexiChain::step`]: sl_client_bevy::FlexiChain::step
const STEP_SETTLE_EPSILON: f32 = 3.0e-4;

/// The squared distance (metres²) the prim's anchor may drift from its settled pose
/// before a latched flexi prim **wakes** (`viewer-perf-flexi-settle-lod`). One
/// millimetre: a prim gliding slower than this per frame accumulates against its
/// recorded rest pose (not the previous frame) and so still wakes once it has moved
/// a millimetre in total, while true rest never trips it.
const WAKE_POSITION_EPSILON_SQ: f32 = 1.0e-3 * 1.0e-3;

/// How far the prim's anchor rotation may turn from its settled orientation (as
/// `1 - |dot|` of the two quaternions) before a latched flexi prim **wakes**. A spin
/// changes the chain's hang direction, so it must re-simulate; the sign-independent
/// `|dot|` treats `q` and `-q` as the same orientation.
const WAKE_ROTATION_EPSILON: f32 = 1.0e-5;

/// How far the prim's metre scale may differ per axis from its settled value before a
/// latched flexi prim **wakes** (`viewer-perf-flexi-settle-lod`). A tenth of a
/// millimetre: below any real resize but enough to avoid an exact float compare.
const WAKE_SCALE_EPSILON: f32 = 1.0e-4;

/// The rest state a **settled** flexi prim is frozen at (`viewer-perf-flexi-settle-lod`):
/// the anchor pose, dequantized attributes, and metre scale that produced its current
/// geometry. [`simulate_flexi`] compares the live values against this each frame and,
/// while they all still match (within [`WAKE_POSITION_EPSILON_SQ`] /
/// [`WAKE_ROTATION_EPSILON`], exactly for attributes / scale), skips the prim
/// entirely — no chain step, no re-tessellation, no GPU upload. Any mismatch (the prim
/// moved, or a script changed its gravity / force / size) wakes it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct FlexiRest {
    /// The prim's anchor world position (SL Z-up metres) at the latch.
    base_position: [f32; 3],
    /// The prim's anchor world rotation `(x, y, z, w)` at the latch.
    base_rotation: [f32; 4],
    /// The dequantized flexi attributes driving the chain at the latch.
    attributes: FlexiAttributes,
    /// The prim's metre scale at the latch.
    scale: [f32; 3],
}

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
    /// The rest state this prim is **settled** and frozen at, or `None` while its
    /// chain is still moving (`viewer-perf-flexi-settle-lod`). `Some` skips the whole
    /// per-frame cost (chain step, re-tessellation, GPU upload) until the prim's pose,
    /// attributes, or scale change; `None` steps and re-uploads every frame until the
    /// chain settles. Seeded `None` so the first frames drive the chain onto its rest
    /// pose before it latches.
    pub(crate) rest: Option<FlexiRest>,
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

/// Whether `current` is within [`WAKE_POSITION_EPSILON_SQ`] of the `rest` anchor
/// position — i.e. the prim has not glided far enough to wake a settled chain.
fn position_settled(current: [f32; 3], rest: [f32; 3]) -> bool {
    let dx = current[0] - rest[0];
    let dy = current[1] - rest[1];
    let dz = current[2] - rest[2];
    dx.mul_add(dx, dy.mul_add(dy, dz * dz)) < WAKE_POSITION_EPSILON_SQ
}

/// Whether the prim's metre `scale` still matches the one it settled at (each axis
/// within [`WAKE_SCALE_EPSILON`]) — i.e. it has not been resized. An exact array
/// compare would flag `clippy::float_cmp`, and a resize is always far larger than
/// this tolerance anyway.
fn scale_settled(current: [f32; 3], rest: [f32; 3]) -> bool {
    (current[0] - rest[0]).abs() < WAKE_SCALE_EPSILON
        && (current[1] - rest[1]).abs() < WAKE_SCALE_EPSILON
        && (current[2] - rest[2]).abs() < WAKE_SCALE_EPSILON
}

/// Whether `current` is within [`WAKE_ROTATION_EPSILON`] of the `rest` anchor
/// rotation — i.e. the prim has not turned far enough to wake a settled chain. Uses
/// the sign-independent `|dot|` so a quaternion and its negation (the same
/// orientation) compare equal.
fn rotation_settled(current: [f32; 4], rest: [f32; 4]) -> bool {
    let dot =
        current[0] * rest[0] + current[1] * rest[1] + current[2] * rest[2] + current[3] * rest[3];
    dot.abs() > 1.0 - WAKE_ROTATION_EPSILON
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
///
/// **Settle latch (`viewer-perf-flexi-settle-detection`).** A settled flexi scene
/// runs the whole per-frame cost — chain step, profile re-tessellation, vertex-buffer
/// re-upload — for dozens of near-static prims (settled hair, skirts, chains, plants)
/// with nothing changing. So once a prim's chain stops moving it is **latched**: the
/// pose / attributes / scale it settled at are recorded in [`FlexiSimState::rest`],
/// and while those inputs still match (the checks below) the prim is skipped entirely
/// — no step, no tessellation, no upload. A prim moves off rest each frame until a
/// [`FlexiChain::step`] reports movement below [`STEP_SETTLE_EPSILON`], when it does
/// one last rewrite and latches. It wakes when the anchor glides
/// ([`WAKE_POSITION_EPSILON_SQ`]) or turns ([`WAKE_ROTATION_EPSILON`]), or a
/// script changes its attributes / scale. The `Aabb` a latched prim keeps (from its
/// last rewrite) stays correct because its geometry is not changing, so frustum
/// culling and ray-cast picking (`viewer-flexi-prim-picking`) still track it.
///
/// [`FlexiChain::step`]: sl_client_bevy::FlexiChain::step
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

        // A settled prim whose inputs are unchanged is frozen — no chain step, no
        // re-tessellation, no GPU upload. Its geometry (and its `Aabb`, so picking /
        // culling) still reflect the rest state it latched at.
        if sim.rest.is_some_and(|rest| {
            rest.attributes == attributes
                && scale_settled(scale, rest.scale)
                && position_settled(base_position, rest.base_position)
                && rotation_settled(base_rotation, rest.base_rotation)
        }) {
            continue;
        }

        let moved = sim
            .chain
            .step(&attributes, scale, base_position, base_rotation, dt);
        // The chain has stopped moving: rewrite the geometry once more, then latch the
        // prim settled at this pose / attributes / scale so it costs nothing until an
        // input changes. While still moving, stay unlatched and re-upload every frame.
        sim.rest = (moved < STEP_SETTLE_EPSILON).then_some(FlexiRest {
            base_position,
            base_rotation,
            attributes,
            scale,
        });

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
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{FlexibleData, Object, Vector};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

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

    /// The full bounds pipeline the flexi pick relies on
    /// (`viewer-flexi-prim-picking`): [`super::simulate_flexi`] rewrites the
    /// face mesh through `Assets::get_mut`, and Bevy's `calculate_bounds` both
    /// inserts the face's missing `Aabb` from the (already deformed) mesh and —
    /// the part a Bevy upgrade could silently break — **refreshes** it via its
    /// `AssetChanged<Mesh3d>` branch as the chain keeps moving. This is why a
    /// flexi face needs no `NoFrustumCulling` opt-out, and why the ray-cast
    /// picks (touch, the object pie) can trust a flexi's `Aabb` to track the
    /// bent geometry.
    #[test]
    fn simulated_flexi_mesh_keeps_its_aabb_fresh() -> Result<(), TestError> {
        use bevy::app::{App, PostUpdate, TaskPoolPlugin, Update};
        use bevy::asset::{
            AssetApp as _, AssetEventSystems, AssetPlugin, Assets, RenderAssetUsages,
        };
        use bevy::camera::primitives::{Aabb, MeshAabb as _};
        use bevy::camera::visibility::calculate_bounds;
        use bevy::ecs::schedule::IntoScheduleConfigs as _;
        use bevy::mesh::{Mesh, Mesh3d, PrimitiveTopology};
        use bevy::time::Time;
        use bevy::transform::components::GlobalTransform;
        use core::time::Duration;
        use sl_client_bevy::{FlexiChain, PrimShapeFloat, PrimShapeParams};

        /// Advance the clock one 50 ms frame and run the app's schedules.
        fn step(app: &mut App) {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(50));
            app.update();
        }

        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Mesh>();
        app.init_resource::<Time>();
        app.add_systems(Update, super::simulate_flexi);
        // The real app gets this system from Bevy's `VisibilityPlugin`; ordered
        // after the asset-event flush so the `AssetChanged` refresh lands the
        // same frame the sim rewrote the mesh.
        app.add_systems(PostUpdate, calculate_bounds.after(AssetEventSystems));

        // A stand-in rest mesh: the sim overwrites the position / normal
        // attributes wholesale, so only its bounds (a centimetre triangle at
        // the origin) matter — as the "stale spawn geometry" the pipeline must
        // replace.
        let rest = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.0, 0.01, 0.0]],
        );
        let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(rest);
        let face = app.world_mut().spawn(Mesh3d(handle.clone())).id();

        // A long, soft chain pushed hard sideways, so every step visibly moves
        // the geometry (a purely axial setup could settle without lateral
        // motion and mask a broken refresh).
        let data = FlexibleData {
            softness: 2,
            tension: 0.5,
            air_friction: 1.0,
            gravity: 1.0,
            wind_sensitivity: 0.0,
            user_force: Vector {
                x: 3.0,
                y: 0.0,
                z: 0.0,
            },
        };
        let shape = PrimShapeFloat::from_params(&PrimShapeParams::default());
        let scale = [0.2, 0.2, 4.0];
        let chain = FlexiChain::new(
            &shape,
            &super::flexi_attributes(&data),
            scale,
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
        );
        app.world_mut().spawn((
            ObjectFlexi { data, scale },
            super::FlexiSimState {
                chain,
                shape,
                softness: 2,
                face_entities: vec![face],
                rest: None,
            },
            GlobalTransform::default(),
        ));

        step(&mut app);
        let first = *app
            .world()
            .get::<Aabb>(face)
            .ok_or("no Aabb after the first simulated frame")?;
        // The insert already reflects the metre-scale swept prim, not the
        // centimetre stand-in triangle.
        assert!(
            first.half_extents.length() > 0.05,
            "spawn Aabb {first:?} still bounds the stand-in triangle"
        );

        for _frame in 0..10 {
            step(&mut app);
        }
        let settled = *app.world().get::<Aabb>(face).ok_or("Aabb lost")?;
        assert_ne!(
            first, settled,
            "the Aabb never refreshed as the chain moved — calculate_bounds' \
             AssetChanged branch no longer covers the flexi rewrite"
        );

        // And the refreshed bounds are exactly the current mesh's own.
        let expected = app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .ok_or("mesh gone")?
            .compute_aabb()
            .ok_or("mesh has no computable Aabb")?;
        assert_eq!(settled, expected);
        Ok(())
    }

    /// The settle latch (`viewer-perf-flexi-settle-lod`): once a stationary flexi
    /// prim's chain has settled, [`super::simulate_flexi`] **freezes** it and stops
    /// rewriting its mesh — the expensive re-tessellation + GPU re-upload are skipped
    /// every frame until an input changes. Proven by overwriting the settled mesh with
    /// a distinctive sentinel and confirming later frames leave it untouched; a still-
    /// running sim would replace it with the swept prim.
    #[test]
    fn a_settled_flexi_freezes_and_stops_rewriting() -> Result<(), TestError> {
        use bevy::app::{App, TaskPoolPlugin, Update};
        use bevy::asset::{AssetApp as _, AssetPlugin, Assets, RenderAssetUsages};
        use bevy::mesh::{Mesh, Mesh3d, PrimitiveTopology, VertexAttributeValues};
        use bevy::time::Time;
        use bevy::transform::components::GlobalTransform;
        use core::time::Duration;
        use sl_client_bevy::{FlexiChain, PrimShapeFloat, PrimShapeParams};

        /// Advance the clock one 16 ms frame (a whole fixed step) and run the app.
        fn step(app: &mut App) {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(16));
            app.update();
        }

        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Mesh>();
        app.init_resource::<Time>();
        app.add_systems(Update, super::simulate_flexi);

        let rest = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.0, 0.01, 0.0]],
        );
        let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(rest);
        let face = app.world_mut().spawn(Mesh3d(handle.clone())).id();

        // A gently-drooping flexi block, so it settles within a handful of frames.
        let data = FlexibleData {
            softness: 2,
            tension: 1.0,
            air_friction: 3.0,
            gravity: 0.3,
            wind_sensitivity: 0.0,
            user_force: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        };
        let shape = PrimShapeFloat::from_params(&PrimShapeParams::default());
        let scale = [0.3, 0.3, 4.0];
        let chain = FlexiChain::new(
            &shape,
            &super::flexi_attributes(&data),
            scale,
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
        );
        app.world_mut().spawn((
            ObjectFlexi { data, scale },
            super::FlexiSimState {
                chain,
                shape,
                softness: 2,
                face_entities: vec![face],
                rest: None,
            },
            GlobalTransform::default(),
        ));

        // Let the chain relax and latch settled (the anchor never moves, so nothing
        // will wake it once it does).
        for _frame in 0..120 {
            step(&mut app);
        }

        // Stamp a distinctive sentinel over the settled geometry.
        let sentinel = vec![[7.0, 7.0, 7.0], [8.0, 8.0, 8.0], [9.0, 9.0, 9.0]];
        {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            let mut mesh = meshes.get_mut(&handle).ok_or("mesh gone")?;
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, sentinel.clone());
        }

        // Run more frames: a frozen prim must not touch the mesh.
        for _frame in 0..20 {
            step(&mut app);
        }

        let mesh = app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .ok_or("mesh gone")?;
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Err("mesh lost its position attribute".into());
        };
        assert_eq!(
            positions, &sentinel,
            "a settled flexi's mesh was rewritten — the settle latch did not freeze it"
        );
        Ok(())
    }

    /// Waking a settled flexi prim (`viewer-perf-flexi-settle-lod`): after it has
    /// latched, **moving its anchor** must un-freeze it so its geometry follows —
    /// otherwise a flexi antenna would stick in place as its wearer walks off. Proven
    /// by settling the prim, stamping a sentinel, teleporting the anchor, and
    /// confirming the sim overwrites the sentinel again.
    #[test]
    fn moving_a_settled_flexi_wakes_it() -> Result<(), TestError> {
        use bevy::app::{App, TaskPoolPlugin, Update};
        use bevy::asset::{AssetApp as _, AssetPlugin, Assets, RenderAssetUsages};
        use bevy::mesh::{Mesh, Mesh3d, PrimitiveTopology, VertexAttributeValues};
        use bevy::time::Time;
        use bevy::transform::components::{GlobalTransform, Transform};
        use core::time::Duration;
        use sl_client_bevy::{FlexiChain, PrimShapeFloat, PrimShapeParams};

        /// Advance the clock one 16 ms frame and run the app.
        fn step(app: &mut App) {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(16));
            app.update();
        }

        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Mesh>();
        app.init_resource::<Time>();
        app.add_systems(Update, super::simulate_flexi);

        let rest = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.0, 0.01, 0.0]],
        );
        let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(rest);
        let face = app.world_mut().spawn(Mesh3d(handle.clone())).id();

        let data = FlexibleData {
            softness: 2,
            tension: 1.0,
            air_friction: 3.0,
            gravity: 0.3,
            wind_sensitivity: 0.0,
            user_force: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        };
        let shape = PrimShapeFloat::from_params(&PrimShapeParams::default());
        let scale = [0.3, 0.3, 4.0];
        let chain = FlexiChain::new(
            &shape,
            &super::flexi_attributes(&data),
            scale,
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
        );
        let prim = app
            .world_mut()
            .spawn((
                ObjectFlexi { data, scale },
                super::FlexiSimState {
                    chain,
                    shape,
                    softness: 2,
                    face_entities: vec![face],
                    rest: None,
                },
                GlobalTransform::default(),
            ))
            .id();

        // Settle.
        for _frame in 0..120 {
            step(&mut app);
        }
        // Sentinel over the settled geometry.
        let sentinel = vec![[7.0, 7.0, 7.0], [8.0, 8.0, 8.0], [9.0, 9.0, 9.0]];
        {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            let mut mesh = meshes.get_mut(&handle).ok_or("mesh gone")?;
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, sentinel.clone());
        }

        // Teleport the prim a couple of metres — well past the wake threshold.
        app.world_mut()
            .entity_mut(prim)
            .insert(GlobalTransform::from(Transform::from_xyz(2.0, 0.0, 0.0)));

        step(&mut app);

        let mesh = app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .ok_or("mesh gone")?;
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Err("mesh lost its position attribute".into());
        };
        assert_ne!(
            positions, &sentinel,
            "moving a settled flexi did not wake it — its geometry is stuck at the old pose"
        );
        Ok(())
    }
}
