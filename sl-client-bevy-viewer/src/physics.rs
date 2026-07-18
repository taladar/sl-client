//! The client-side physics foundation (P31.1): an [`avian3d`] physics world
//! bridged into the viewer's Bevy Y-up scene.
//!
//! This module stands up a shared physics substrate that later phases build on
//! — Phase 32 (flexi prims) and Phase 34 (avatar cloth / body physics) drive
//! their client-side simulations through it rather than hand-rolling a solver.
//! P31.1 stood up the world; P31.2 (below) populates it with the server-flagged
//! physical prims. The world is configured with three foundation pieces:
//!
//! - **Second Life gravity.** Second Life's world gravity is `-9.8` m/s² along
//!   its Z (up) axis (Firestorm `llmath.h` `GRAVITY = -9.8`, matched by
//!   OpenSim's `world_gravityz = -9.8`). The single Second Life → Bevy basis
//!   change (see [`crate::coords`]) carries that Z-up vector into Bevy's Y-up
//!   frame, so avian's [`Gravity`] resource points along `-Y`.
//! - **A fixed timestep.** avian runs its schedule in `FixedPostUpdate`, driven
//!   by Bevy's `Time<Fixed>` clock; pinning that clock to the Second Life
//!   simulator's target physics rate ([`SL_PHYSICS_HZ`], 45 Hz) makes the
//!   client's physics advance at the sim's cadence.
//! - **Time dilation.** A laden region does not keep up with 45 Hz; it reports
//!   the fraction of real time its physics frame is achieving in the
//!   `RegionData.TimeDilation` field of every object-update message (surfaced as
//!   [`SlSessionEvent::TimeDilation`]). avian's *relative speed* is exactly a
//!   time-dilation control, so [`drive_physics_time_dilation`] scales the
//!   physics clock by the agent region's dilation each frame — client-side
//!   dynamics then slow down in lock-step with the dilated sim instead of
//!   drifting ahead of it.
//!
//! **P31.2 — physical objects.** Every server-flagged physical root prim
//! ([`FLAGS_USE_PHYSICS`], marked by [`apply_physics`] from
//! [`apply_object`](crate::objects)) gets a **kinematic** avian [`RigidBody`] and
//! a cuboid [`Collider`] sized to its prim scale. The simulator stays
//! authoritative: [`drive_physical_objects`] snaps the body to each
//! `ObjectUpdate` and, between updates, dead-reckons the pose forward exactly as
//! the reference viewer's `LLViewerObject::interpolateLinearMotion` does — the
//! velocity/acceleration extrapolation, the circuit-health phase-out (easing a
//! silent object to a halt once the circuit looks stalled), and the geometric
//! clamps (region-height ceiling, permissive ground floor, off-region-edge clip,
//! region-crossing cap). The body is **never** free-run under the world gravity,
//! so a settled object the sim has gone silent about cannot drift; avian's
//! genuine dynamic bodies + [`Gravity`] are reserved for client-only motion
//! (Phases 32 / 34). This is why the guards matter: silence means "prediction
//! still holds", so a bounded, corrected extrapolation is the faithful model.
//!
//! **P31.3 — physics-shape-aware colliders.** P31.2's collider is a placeholder
//! cuboid sized to the prim scale, regardless of the object's real collision
//! shape. P31.3 replaces it with a collider that matches the simulator's
//! `LLPhysicsShapeType`, fetched via the `GetObjectPhysicsData` capability
//! ([`Command::RequestObjectPhysicsData`], requested by
//! [`request_object_physics_data`] the first time an object is flagged physical)
//! and folded — together with any unsolicited `ObjectPhysicsProperties` event
//! pushes — into [`ObjectPhysicsShapes`] by [`ingest_object_physics_data`]. Once a
//! shape type is known, [`refine_physical_colliders`] builds the matching avian
//! [`Collider`] from the geometry the viewer already tessellates (gathered from the
//! object's own [`GeometryHolder`] faces, so linkset children are excluded):
//! **none** → no collider; **convex hull** → [`Collider::convex_hull`] of the prim
//! / mesh vertices; **prim** → a [`Collider::trimesh`] of that geometry. Until the
//! shape data and the geometry are both available, the placeholder cuboid stands
//! in. These colliders are inert on the kinematic movers themselves — they matter
//! once Phases 32 / 34 add genuine dynamic bodies that collide against them.
//!
//! **P31.4 — avatar dead-reckoning.** The same `interpolateLinearMotion` port is
//! extended to the own and other full-object avatars (the [`crate::avatars`] path,
//! not the object path): [`apply_object`](crate::avatars) stamps each avatar's
//! anchor with an [`AvatarMotion`] marker, and [`drive_avatar_motion`] dead-reckons
//! it between updates with the same phase-out taper and geometric clamps — but with
//! the **stricter avatar ground floor** ([`avatar_ground_floor`]:
//! `land + 0.5 * height`) so a laggy avatar does not sink under the terrain. The
//! shared [`MotionState`] + [`advance_motion`] step drive both the object and avatar
//! paths; they differ only in that ground floor. Avatars stay kinematic
//! (sim-authoritative); the predicted motion is applied to the anchor as a
//! translation delta so the pelvis / shoe render offset is preserved.
//!
//! No `sl-client-tokio` counterpart is needed: like the render materials and the
//! other viewer-only simulations (sky, water, particles), the physics world is a
//! viewer rendering concern, not a protocol capability, so the runtime-parity
//! rule does not apply.

use std::collections::{HashMap, HashSet};

use avian3d::physics_transform::PhysicsTransformConfig;
use avian3d::prelude::{Collider, Gravity, Physics, PhysicsPlugins, PhysicsTime as _, RigidBody};
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use sl_client_bevy::{
    Command, Object, ObjectKey, ObjectPhysicsData, PhysicsShapeType, RegionHandle, Rotation,
    SlCommand, SlEvent, SlIdentity, SlSessionEvent, Vector,
};

use crate::avatars::update_avatar_objects;
use crate::coords::{sl_rotation_to_quat, sl_to_bevy_rotation, sl_to_bevy_vec};
use crate::objects::{GeometryHolder, ObjectState, update_objects};
use crate::terrain::TerrainState;

/// Second Life's world gravity along its Z (up) axis, in metres per second
/// squared (Firestorm `llmath.h` `GRAVITY = -9.8`; OpenSim `world_gravityz =
/// -9.8`).
const SL_GRAVITY_Z: f32 = -9.8;

/// The Second Life simulator's target physics frame rate, in hertz. The sim
/// runs its physics at 45 frames per second when fully keeping up; the client's
/// physics world uses the same fixed step so its client-side dynamics advance at
/// the sim's cadence. The actual achieved rate is scaled down by the region's
/// time dilation (see [`drive_physics_time_dilation`]).
const SL_PHYSICS_HZ: f64 = 45.0;

/// The world gravity avian uses, as a Bevy Y-up vector: Second Life's `-9.8`
/// m/s² Z-up gravity carried through the single Second Life → Bevy basis change,
/// landing along Bevy `-Y`.
#[must_use]
fn sl_gravity() -> Vec3 {
    sl_to_bevy_vec(&Vector {
        x: 0.0,
        y: 0.0,
        z: SL_GRAVITY_Z,
    })
}

/// Clamp a raw region time dilation into avian's *relative speed* contract.
///
/// `set_relative_speed` panics on a negative or non-finite ratio; the wire value
/// is already `0.0..=1.0`, but guard a non-finite value (falling back to a
/// healthy `1.0`) and clamp into range so a malformed update can never poison the
/// physics clock.
#[must_use]
const fn clamp_dilation(dilation: f32) -> f32 {
    if dilation.is_finite() {
        dilation.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// The most recent `RegionData.TimeDilation` seen per region, folded from the
/// session event stream by [`ingest_time_dilation`] and read (for the agent's
/// current region) by [`drive_physics_time_dilation`].
#[derive(Resource, Default)]
pub(crate) struct RegionTimeDilation {
    /// The latest dilation (`0.0..=1.0`) for each region, keyed by handle.
    per_region: HashMap<RegionHandle, f32>,
}

/// The viewer's physics plugin: adds the avian physics world, sets Second Life
/// gravity + the fixed timestep, and wires the time-dilation drive.
pub(crate) struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::default())
            // Second Life gravity, bridged into Bevy's Y-up frame.
            .insert_resource(Gravity(sl_gravity()))
            // Do NOT let avian re-run Bevy's *general* transform propagation before it
            // steps physics. By default avian propagates every entity's `Transform` →
            // `GlobalTransform` in its (45 Hz `FixedPostUpdate`) schedule so a body's
            // world pose is current before the sim reads it. That extra pass runs inside
            // `RunFixedMainLoop`, before `Update`, and recomputes the avatar joints'
            // `GlobalTransform`s from their rest local transforms — clobbering the
            // animated globals `pose_avatar_skeletons` writes directly (P18.3). Because
            // that pass fires only on frames that run a fixed step (3 of every 4 at
            // 45 Hz vs a 60 Hz display), anything reading a joint global in `Update` —
            // the third-person camera focus and the foot-IK ground probe — saw the head
            // flicker between the rest and animated pose, a whole-body vibration the
            // head-following camera made obvious (`viewer-avatar-motion-render-smoothing`).
            // We have no dynamic bodies (physical prims are kinematic movers we snap each
            // frame; their colliders are inert until Phase 32/34 add real dynamics), so
            // this pre-physics propagation is dead work for us — Bevy's own `PostUpdate`
            // propagation still keeps every physics body's `GlobalTransform` current for
            // rendering and for the next frame's sim. Turning it off removes the clobber
            // at its source; the rendered avatar was always correct (its pose is written
            // last in `PostUpdate`, after this pass), only the `Update` readers saw the
            // transient.
            .insert_resource(PhysicsTransformConfig {
                propagate_before_physics: false,
                ..Default::default()
            })
            // Pin the fixed clock that drives avian's `FixedPostUpdate` schedule
            // to the Second Life simulator's target physics rate.
            .insert_resource(Time::<Fixed>::from_hz(SL_PHYSICS_HZ))
            .init_resource::<RegionTimeDilation>()
            .init_resource::<CircuitLiveness>()
            .init_resource::<ObjectPhysicsShapes>()
            .add_systems(
                Update,
                (
                    (ingest_time_dilation, drive_physics_time_dilation).chain(),
                    ingest_circuit_liveness,
                    // P31.3: fold the object physics-shape data (capability reply +
                    // event-queue pushes) and request it for newly-physical objects.
                    ingest_object_physics_data,
                    request_object_physics_data,
                    // P31.2: give server-flagged physical prims a kinematic body and
                    // dead-reckon them between updates. Runs after `update_objects`
                    // has attached / refreshed the [`PhysicalObject`] marker.
                    drive_physical_objects.after(update_objects),
                    detach_physical_bodies.after(update_objects),
                    // P31.4: dead-reckon avatars between updates (the avatars.rs
                    // path), after `apply_object` has refreshed the [`AvatarMotion`].
                    drive_avatar_motion.after(update_avatar_objects),
                    // P31.3: replace the placeholder cuboid with a shape-aware
                    // collider once the physics data and geometry are available.
                    refine_physical_colliders
                        .after(update_objects)
                        .after(drive_physical_objects)
                        .after(ingest_object_physics_data),
                ),
            );
    }
}

/// Fold each region's most recent `TimeDilation` into [`RegionTimeDilation`].
pub(crate) fn ingest_time_dilation(
    mut events: MessageReader<SlEvent>,
    mut dilations: ResMut<RegionTimeDilation>,
) {
    for event in events.read() {
        if let SlSessionEvent::TimeDilation {
            region_handle,
            dilation,
        } = &event.0
        {
            dilations.per_region.insert(*region_handle, *dilation);
        }
    }
}

/// Scale the physics clock by the agent's current-region time dilation each
/// frame, so client-side physics runs at the same effective rate as the (laden)
/// simulator. Defaults to full speed (`1.0`) while the region is unknown or has
/// not yet reported a dilation.
pub(crate) fn drive_physics_time_dilation(
    identity: Res<SlIdentity>,
    dilations: Res<RegionTimeDilation>,
    mut time: ResMut<Time<Physics>>,
) {
    let dilation = identity
        .region_handle
        .and_then(|handle| dilations.per_region.get(&handle).copied())
        .unwrap_or(1.0);
    let ratio = clamp_dilation(dilation);
    // Only touch the clock when the ratio actually changes, so a steady region
    // does no per-frame resource mutation.
    if (time.relative_speed() - ratio).abs() > f32::EPSILON {
        time.set_relative_speed(ratio);
    }
}

/// The `FLAGS_USE_PHYSICS` bit of an object's update flags (`object_flags.h`):
/// the object is simulated by the server's physics engine. This is the "physical
/// object" flag the reference viewer reads (`LLViewerObject::flagUsePhysics`).
const FLAGS_USE_PHYSICS: u32 = 1 << 0;

/// The simulator's physics timestep, in seconds (`llviewerobject.cpp`
/// `PHYSICS_TIMESTEP = 1/45`). The dead-reckoning correction below uses it to
/// account for the fact that an object update's velocity is the *average* over
/// the last step rather than the final velocity.
const PHYSICS_TIMESTEP: f32 = 1.0 / 45.0;

/// Seconds of silence after which motion prediction begins to taper off
/// (`sPhaseOutUpdateInterpolationTime`), *if* the circuit also looks stalled.
const PHASE_OUT_START_SECS: f64 = 2.0;

/// Seconds of silence after which motion prediction is fully off
/// (`sMaxUpdateInterpolationTime`) — the object is eased to a halt by then.
const MAX_INTERP_SECS: f64 = 3.0;

/// The tighter interpolation cap while an object is predicted to be crossing a
/// region border (`sMaxRegionCrossingInterpolationTime`) — the classic "shot off
/// across the region" source is bounded to a second.
const REGION_CROSSING_CAP_SECS: f64 = 1.0;

/// A standard region's edge length, in metres. Variable-region sizes are out of
/// scope here; the off-region-edge clip below assumes the 256 m grid the local
/// test grid and Second Life mainland use.
const REGION_WIDTH_M: f32 = 256.0;

/// The region-height ceiling an extrapolated object is clamped under
/// (`getRegionMaxHeight`). Second Life's `SL_MAX_OBJECT_Z`; OpenSim's is higher
/// (`OS_MAX_OBJECT_Z = 10000`), but this only bounds runaway *prediction* — an
/// authoritative update always reseeds the true position first — and Second Life
/// is the primary target.
const REGION_MAX_HEIGHT_M: f32 = 4096.0;

/// The smallest collider extent, in metres, so a prim with a degenerate (zero)
/// scale axis still gets a valid, non-panicking avian cuboid.
const MIN_COLLIDER_EXTENT_M: f32 = 0.01;

/// Whether `object` is a server-flagged **physical root prim** the viewer drives
/// kinematically: it carries [`FLAGS_USE_PHYSICS`], is a linkset root (its
/// children ride along via the Bevy hierarchy), and is not a worn attachment (the
/// reference viewer skips linear interpolation for attachments — they follow
/// their wearer's skeleton joint instead).
const fn is_physical_root(object: &Object) -> bool {
    object.update_flags & FLAGS_USE_PHYSICS != 0
        && object.parent_id.get() == 0
        && object.attachment_point_id().is_none()
}

/// The authoritative kinematic state of a server-flagged physical root prim as of
/// its last `ObjectUpdate`, attached to the object entity by [`apply_physics`] and
/// change-detected: a fresh insert on every update reseeds the interpolation. The
/// component is absent on any object that is not a physical root, so its presence
/// alone marks the entities [`drive_physical_objects`] gives a kinematic body.
#[derive(Component, Clone)]
pub(crate) struct PhysicalObject {
    /// The object's full (grid-wide) key — the id the `GetObjectPhysicsData`
    /// capability request and its reply use, and the key
    /// [`ObjectPhysicsShapes`] stores this object's physics data under.
    full_key: ObjectKey,
    /// Region-local position (metres, Second Life Z-up frame).
    position: Vector,
    /// Linear velocity (metres/second).
    velocity: Vector,
    /// Linear acceleration (metres/second²) — usually gravity for a falling prim.
    acceleration: Vector,
    /// Orientation (a Second Life unit quaternion).
    rotation: Rotation,
    /// Angular velocity (rotation axis scaled by radians/second).
    angular_velocity: Vector,
    /// The region this object lives in, for the region-edge / neighbour lookups.
    region_handle: RegionHandle,
    /// The object's size (metres per axis), the source for its cuboid collider.
    scale: Vector,
}

/// Attach, refresh, or remove the [`PhysicalObject`] marker on an object entity to
/// match its current physical-root status — the physics counterpart of
/// [`apply_light`](crate::lights) / `apply_particles`, called from
/// [`apply_object`](crate::objects) on every add and update so a prim toggled
/// physical / non-physical (or moved by a terse update) is reflected. The avian
/// [`RigidBody`] / [`Collider`] themselves are managed by
/// [`drive_physical_objects`] from this marker's presence.
pub(crate) fn apply_physics(entity: Entity, object: &Object, commands: &mut Commands) {
    if is_physical_root(object) {
        commands.entity(entity).insert(PhysicalObject {
            full_key: object.full_id,
            position: object.motion.position.clone(),
            velocity: object.motion.velocity.clone(),
            acceleration: object.motion.acceleration.clone(),
            rotation: object.motion.rotation.clone(),
            angular_velocity: object.motion.angular_velocity.clone(),
            region_handle: object.region_handle,
            scale: object.scale.clone(),
        });
    } else {
        commands.entity(entity).remove::<PhysicalObject>();
    }
}

/// The evolving dead-reckoning prediction shared by the object
/// ([`PhysicsInterp`]) and avatar ([`AvatarInterp`]) motion drivers: the
/// extrapolated (predicted) region-local pose plus the motion state that
/// [`advance_motion`] steps forward each frame between authoritative server
/// updates. All of it is in Second Life space (Z-up, pre basis-change), so the
/// same math serves both paths — they differ only in the ground floor they apply
/// (permissive for objects, stricter for avatars).
struct MotionState {
    /// The predicted region-local position (Second Life Z-up metres).
    position: [f32; 3],
    /// The predicted orientation, in Second Life space (pre basis-change).
    rotation: Quat,
    /// The current linear velocity (metres/second), decaying under the phase-out.
    velocity: [f32; 3],
    /// The current linear acceleration (metres/second²); zeroed on a region cross
    /// or an empty-edge clip, matching the reference viewer.
    acceleration: [f32; 3],
    /// The angular velocity (axis·radians/second).
    angular_velocity: [f32; 3],
    /// The object's / avatar's region, for the region-edge / neighbour lookups.
    region_handle: RegionHandle,
    /// While predicted to be crossing a border, the elapsed-seconds deadline after
    /// which motion is stopped (`mRegionCrossExpire`); `None` when not crossing.
    region_cross_expire: Option<f64>,
}

impl MotionState {
    /// Seed the prediction from an authoritative update's motion fields.
    fn new(
        position: &Vector,
        velocity: &Vector,
        acceleration: &Vector,
        rotation: &Rotation,
        angular_velocity: &Vector,
        region_handle: RegionHandle,
    ) -> Self {
        Self {
            position: vector_to_array(position),
            rotation: sl_rotation_to_quat(rotation),
            velocity: vector_to_array(velocity),
            acceleration: vector_to_array(acceleration),
            angular_velocity: vector_to_array(angular_velocity),
            region_handle,
            region_cross_expire: None,
        }
    }
}

/// Advance a [`MotionState`] one dead-reckoning frame, exactly as the reference
/// viewer's `LLViewerObject::interpolateLinearMotion` does: extrapolate the
/// linear motion (only for an actually-moving body — the reference's
/// `!accel.isExactlyZero() || !vel.isExactlyZero()` gate), apply the geometric
/// clamps, then spin the orientation by the angular velocity. `floor_at` resolves
/// the ground floor for the *predicted* horizontal position — the object and
/// avatar paths differ only in this floor, so the caller supplies it.
fn advance_motion<F>(
    motion: &mut MotionState,
    neighbours: [bool; 4],
    dt: f32,
    phase_out: f32,
    now: f64,
    floor_at: F,
) where
    F: FnOnce(f32, f32) -> Option<f32>,
{
    let moving = motion
        .velocity
        .iter()
        .chain(&motion.acceleration)
        .any(|c| c.abs() > f32::EPSILON);
    if moving {
        let (predicted, velocity) = dead_reckon(
            motion.position,
            motion.velocity,
            motion.acceleration,
            dt,
            phase_out,
        );
        let [predicted_x, predicted_y, _] = predicted;
        let clamped = clamp_prediction(ClampInput {
            position: predicted,
            velocity,
            acceleration: motion.acceleration,
            floor: floor_at(predicted_x, predicted_y),
            neighbours,
            region_cross_expire: motion.region_cross_expire,
            now,
        });
        motion.position = clamped.position;
        motion.velocity = clamped.velocity;
        motion.acceleration = clamped.acceleration;
        motion.region_cross_expire = clamped.region_cross_expire;
    }
    // Angular velocity is applied even for a purely spinning body.
    motion.rotation = angular_step(motion.rotation, motion.angular_velocity, dt);
}

/// The viewer-side interpolation state for one physical object, owned entirely by
/// [`drive_physical_objects`]: the extrapolated (predicted) pose advanced each
/// frame between server updates, the timing the phase-out reads, and the collider
/// scale (to rebuild the cuboid only on a genuine resize). Mirrors the reference
/// viewer's per-`LLViewerObject` interpolation bookkeeping.
#[derive(Component)]
pub(crate) struct PhysicsInterp {
    /// The shared dead-reckoning prediction (pose + motion) advanced each frame.
    motion: MotionState,
    /// Elapsed seconds when the last server update was ingested
    /// (`mLastMessageUpdateSecs`).
    last_message_secs: f64,
    /// Elapsed seconds at the last interpolation step (`mLastInterpUpdateSecs`).
    last_interp_secs: f64,
    /// The collider's current extents (metres), to detect a resize.
    collider_scale: [f32; 3],
}

impl PhysicsInterp {
    /// Seed the interpolation state from an authoritative update at time `now`.
    fn seeded(phys: &PhysicalObject, now: f64) -> Self {
        Self {
            motion: MotionState::new(
                &phys.position,
                &phys.velocity,
                &phys.acceleration,
                &phys.rotation,
                &phys.angular_velocity,
                phys.region_handle,
            ),
            last_message_secs: now,
            last_interp_secs: now,
            collider_scale: collider_extents(&phys.scale),
        }
    }

    /// Re-seed the predicted pose to a fresh authoritative update at time `now`,
    /// snapping the prediction back to the server truth and restarting the timers.
    fn reseed(&mut self, phys: &PhysicalObject, now: f64) {
        self.motion = MotionState::new(
            &phys.position,
            &phys.velocity,
            &phys.acceleration,
            &phys.rotation,
            &phys.angular_velocity,
            phys.region_handle,
        );
        self.last_message_secs = now;
        self.last_interp_secs = now;
        // Keep the cached scale current so the ground-floor bounding radius (and a
        // later collider resize by [`refine_physical_colliders`]) track a resize.
        self.collider_scale = collider_extents(&phys.scale);
    }
}

/// A [`Vector`]'s components as a plain `[f32; 3]` for the per-component
/// dead-reckoning math (Bevy's `Vec3` arithmetic operators are forbidden by the
/// workspace `arithmetic_side_effects` lint — see [`crate::camera`]).
const fn vector_to_array(vector: &Vector) -> [f32; 3] {
    [vector.x, vector.y, vector.z]
}

/// The cuboid collider extents for a prim scale, each floored to a valid
/// non-degenerate length.
const fn collider_extents(scale: &Vector) -> [f32; 3] {
    [
        scale.x.max(MIN_COLLIDER_EXTENT_M),
        scale.y.max(MIN_COLLIDER_EXTENT_M),
        scale.z.max(MIN_COLLIDER_EXTENT_M),
    ]
}

/// The last elapsed-seconds time any inbound session event was seen, a proxy for
/// the reference viewer's per-circuit last-packet time (`getLastPacketInTime`):
/// the phase-out taper only engages once this goes stale, separating "quiet
/// because the prediction is right" from "quiet because the sim is lagging".
#[derive(Resource, Default)]
pub(crate) struct CircuitLiveness {
    /// Elapsed seconds at the most recent inbound [`SlEvent`], or `None` before
    /// any event has arrived (treated as freshly alive).
    last_event_secs: Option<f64>,
}

/// Refresh [`CircuitLiveness`] whenever any inbound session event arrives: a
/// healthy circuit keeps a steady stream flowing (object, terrain, ping, …), so a
/// stale timestamp means the circuit — not just one silent object — has gone
/// quiet, which is exactly when the reference viewer tapers off prediction.
pub(crate) fn ingest_circuit_liveness(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    mut liveness: ResMut<CircuitLiveness>,
) {
    // Drain the frame's events (advancing the cursor); any inbound traffic marks
    // the circuit alive right now.
    if events.read().count() > 0 {
        liveness.last_event_secs = Some(time.elapsed_secs_f64());
    }
}

/// The `getMinAllowedZ`-style ground floor for a physical object: the land height
/// under it minus the object's bounding radius (half its scale length). The
/// reference viewer deliberately keeps this permissive for objects (they may sink
/// underground) — it only stops a laggy prediction running arbitrarily far below
/// the terrain. `None` land height (terrain not yet ingested) means no floor.
fn ground_floor(land_height: Option<f32>, scale: &Vector) -> Option<f32> {
    land_height.map(|height| {
        let radius = 0.5 * (scale.x * scale.x + scale.y * scale.y + scale.z * scale.z).sqrt();
        height - radius
    })
}

/// The interpolation phase-out factor (`1.0` full prediction … `0.0` stopped),
/// reproducing `LLViewerObject::interpolateLinearMotion`'s ramp. `now`-relative
/// times are elapsed seconds; `circuit_stale` is whether the circuit looks lagged
/// (only then does prediction taper — otherwise silence means the prediction is
/// still correct and we keep going at `1.0`).
fn phase_out_factor(
    time_since_last_update: f64,
    time_since_last_interp: f64,
    last_update_already_phased: bool,
    circuit_stale: bool,
) -> f64 {
    if time_since_last_update <= PHASE_OUT_START_SECS || !circuit_stale {
        return 1.0;
    }
    if time_since_last_update > MAX_INTERP_SECS {
        // Past the limit: stop the object.
        return 0.0;
    }
    let raw = if last_update_already_phased {
        // The previous step was already tapering: ramp relative to it.
        let denom = MAX_INTERP_SECS - time_since_last_interp;
        if denom.abs() < f64::EPSILON {
            1.0
        } else {
            (MAX_INTERP_SECS - time_since_last_update) / denom
        }
    } else {
        // Start the taper from the full value.
        (MAX_INTERP_SECS - time_since_last_update) / (MAX_INTERP_SECS - PHASE_OUT_START_SECS)
    };
    raw.clamp(0.0, 1.0)
}

/// Advance a predicted position/velocity one dead-reckoning step, reproducing the
/// reference viewer's `new_pos = (vel + 0.5*(dt - PHYSICS_TIMESTEP)*accel) * dt`
/// (scaled by the phase-out), returning the new `(position, velocity)`.
fn dead_reckon(
    position: [f32; 3],
    velocity: [f32; 3],
    acceleration: [f32; 3],
    dt: f32,
    phase_out: f32,
) -> ([f32; 3], [f32; 3]) {
    let half_correction = 0.5 * (dt - PHYSICS_TIMESTEP);
    let [px, py, pz] = position;
    let [vx, vy, vz] = velocity;
    let [ax, ay, az] = acceleration;
    // One axis's predicted `(position, velocity)` step.
    let step = |p: f32, v: f32, a: f32| -> (f32, f32) {
        let delta = (v + half_correction * a) * dt * phase_out;
        (p + delta, v + a * dt * phase_out)
    };
    let (npx, nvx) = step(px, vx, ax);
    let (npy, nvy) = step(py, vy, ay);
    let (npz, nvz) = step(pz, vz, az);
    ([npx, npy, npz], [nvx, nvy, nvz])
}

/// Advance an orientation by its angular velocity over `dt`, reproducing
/// `LLViewerObject::applyAngularVelocity` (a delta quaternion about the normalised
/// angular-velocity axis). A near-zero angular velocity leaves the rotation
/// unchanged.
fn angular_step(rotation: Quat, angular_velocity: [f32; 3], dt: f32) -> Quat {
    let [ax, ay, az] = angular_velocity;
    let omega_sq = ax * ax + ay * ay + az * az;
    if omega_sq <= 1.0e-8 {
        return rotation;
    }
    let omega = omega_sq.sqrt();
    let angle = omega * dt;
    let axis = Vec3::new(ax / omega, ay / omega, az / omega);
    rotation
        .mul_quat(Quat::from_axis_angle(axis, angle))
        .normalize()
}

/// The Bevy-world orientation of a predicted motion: its Second Life-space rotation
/// composed with the Second Life → Bevy basis change, matching the root transform
/// [`body_root_transform`](crate::avatars) writes on an authoritative update.
fn bevy_rotation_of(motion: &MotionState) -> Quat {
    sl_to_bevy_rotation().mul_quat(motion.rotation)
}

/// Exponential-smoothing time constant (seconds) for easing the avatar's *rendered*
/// orientation toward its authoritative / dead-reckoned facing (P31.7). The own
/// avatar's facing arrives only as sparse `ObjectUpdate`s echoing the client-driven
/// `SetRotation` (P31.5, throttled to ~20 Hz and coarser once the sim re-broadcasts
/// it), so the target jumps in steps; a ~80 ms constant smooths those steps into a
/// fluid turn while staying responsive (it covers ~63 % of a step in one constant,
/// ~95 % in three) and converges to the target once turning stops, leaving no
/// standing lag.
const ROTATION_SMOOTHING_TAU_SECS: f32 = 0.08;

/// The exponential-smoothing blend factor for a frame of length `dt` seconds
/// (`1 - e^(-dt/τ)`), the framerate-independent easing toward the target facing. A
/// non-positive `dt` blends fully (snap) so a paused / first frame cannot stall.
fn rotation_smoothing_alpha(dt: f32) -> f32 {
    if dt <= 0.0 {
        return 1.0;
    }
    1.0 - (-dt / ROTATION_SMOOTHING_TAU_SECS).exp()
}

/// Ease the avatar anchor's rendered orientation toward its current authoritative /
/// dead-reckoned facing and write it to the anchor (P31.7). A no-op for a
/// placeholder sphere (which does not carry the object rotation), so only rigged
/// bodies smooth-turn. `dt` is the real (undilated) frame time — the smoothing is a
/// visual concern, independent of the physics clock.
fn apply_smoothed_rotation(interp: &mut AvatarInterp, transform: &mut Transform, dt: f32) {
    if !interp.apply_rotation {
        return;
    }
    let target = bevy_rotation_of(&interp.motion);
    let alpha = rotation_smoothing_alpha(dt);
    interp.rendered_rotation = interp.rendered_rotation.slerp(target, alpha);
    transform.rotation = interp.rendered_rotation;
}

/// Which of the four axis-neighbour regions (`[-x, +x, -y, +y]`) are currently
/// known (a circuit / terrain seen for them), from the regions the session has
/// reported a time dilation for — the analogue of the reference viewer's
/// `clipToVisibleRegions`.
fn neighbours_known(dilations: &RegionTimeDilation, region: RegionHandle) -> [bool; 4] {
    let (gx, gy) = region.global_coordinates();
    let width = 256_u32;
    let known = |x: Option<u32>, y: Option<u32>| match (x, y) {
        (Some(x), Some(y)) => dilations
            .per_region
            .contains_key(&RegionHandle::from_global(x, y)),
        _ => false,
    };
    [
        known(gx.checked_sub(width), Some(gy)),
        known(gx.checked_add(width), Some(gy)),
        known(Some(gx), gy.checked_sub(width)),
        known(Some(gx), gy.checked_add(width)),
    ]
}

/// The inputs to [`clamp_prediction`]: an extrapolated pose plus the world facts
/// its guards need.
struct ClampInput {
    /// The extrapolated region-local position (Second Life Z-up metres).
    position: [f32; 3],
    /// The extrapolated linear velocity (metres/second).
    velocity: [f32; 3],
    /// The current linear acceleration (metres/second²).
    acceleration: [f32; 3],
    /// The ground floor to clamp the height above, or `None` for no floor.
    floor: Option<f32>,
    /// Which axis-neighbour regions are known (`[-x, +x, -y, +y]`).
    neighbours: [bool; 4],
    /// The current region-cross deadline (elapsed seconds), if crossing.
    region_cross_expire: Option<f64>,
    /// The current time (elapsed seconds).
    now: f64,
}

/// The result of [`clamp_prediction`]: the clamped pose and the (possibly zeroed)
/// motion state to store back.
struct ClampOutput {
    /// The clamped region-local position.
    position: [f32; 3],
    /// The velocity to store (zeroed on an empty-edge clip / crossing timeout).
    velocity: [f32; 3],
    /// The acceleration to store (zeroed on an empty-edge clip or a crossing).
    acceleration: [f32; 3],
    /// The updated region-cross deadline.
    region_cross_expire: Option<f64>,
}

/// The clamp result for one horizontal axis: its clamped coordinate, whether it
/// left the region into a void (an empty edge), and whether it left into a known
/// neighbour (a border crossing).
struct AxisClip {
    /// The coordinate after an empty-edge clip (unchanged when in-region or
    /// crossing into a neighbour).
    coordinate: f32,
    /// Left the region with no neighbour to enter.
    into_void: bool,
    /// Left the region into a known neighbour.
    crossing: bool,
}

/// Clip one horizontal coordinate against the region bounds, given whether the
/// lower / upper neighbour region is known.
fn clip_axis(coordinate: f32, lower_known: bool, upper_known: bool) -> AxisClip {
    if coordinate < 0.0 {
        if lower_known {
            AxisClip {
                coordinate,
                into_void: false,
                crossing: true,
            }
        } else {
            AxisClip {
                coordinate: 0.0,
                into_void: true,
                crossing: false,
            }
        }
    } else if coordinate > REGION_WIDTH_M {
        if upper_known {
            AxisClip {
                coordinate,
                into_void: false,
                crossing: true,
            }
        } else {
            AxisClip {
                coordinate: REGION_WIDTH_M,
                into_void: true,
                crossing: false,
            }
        }
    } else {
        AxisClip {
            coordinate,
            into_void: false,
            crossing: false,
        }
    }
}

/// The geometric guards on an extrapolated step, reproducing the reference
/// viewer's clamps: a region-height ceiling, a (permissive) ground floor, and the
/// off-region-edge clip / region-crossing cap. Returns the clamped position and
/// the (possibly zeroed) velocity / acceleration / region-cross deadline to store.
///
/// - Leaving the region into a **void** (no known neighbour): clip to the edge and
///   zero velocity + acceleration, waiting for a server update.
/// - Leaving into a **known neighbour**: a border crossing — zero acceleration and
///   bound the crossing to [`REGION_CROSSING_CAP_SECS`], stopping motion past it.
fn clamp_prediction(input: ClampInput) -> ClampOutput {
    let [x, y, z] = input.position;
    let mut velocity = input.velocity;
    let mut acceleration = input.acceleration;

    // Region-height ceiling and (permissive) ground floor.
    let mut clamped_z = z.min(REGION_MAX_HEIGHT_M);
    if let Some(floor) = input.floor {
        clamped_z = clamped_z.max(floor);
    }

    // Off-region-edge clip, per horizontal axis. `neighbours` is `[-x, +x, -y, +y]`.
    let [neg_x, pos_x, neg_y, pos_y] = input.neighbours;
    let clip_x = clip_axis(x, neg_x, pos_x);
    let clip_y = clip_axis(y, neg_y, pos_y);
    let position = [clip_x.coordinate, clip_y.coordinate, clamped_z];
    let into_void = clip_x.into_void || clip_y.into_void;
    let crossing = clip_x.crossing || clip_y.crossing;

    let mut region_cross_expire = input.region_cross_expire;
    if into_void {
        // Hit an empty region edge: stop motion and wait for a server update.
        velocity = [0.0; 3];
        acceleration = [0.0; 3];
        region_cross_expire = None;
    } else if crossing {
        // A predicted border crossing: no acceleration while crossing, and bound
        // the extrapolation to a second so a laggy crossing does not shoot off.
        acceleration = [0.0; 3];
        match region_cross_expire {
            None => region_cross_expire = Some(input.now + REGION_CROSSING_CAP_SECS),
            Some(expire) if input.now > expire => {
                velocity = [0.0; 3];
                region_cross_expire = None;
            }
            Some(_) => {}
        }
    } else {
        region_cross_expire = None;
    }

    ClampOutput {
        position,
        velocity,
        acceleration,
        region_cross_expire,
    }
}

/// Give each server-flagged physical prim a kinematic avian body and drive it: on
/// the frame an [`PhysicalObject`] update lands, snap to the authoritative pose
/// and (re)seed the interpolation; between updates, dead-reckon the pose forward
/// with the phase-out taper and the geometric clamps, exactly as the reference
/// viewer's `interpolateLinearMotion` does. The body stays **kinematic** (the sim
/// is authoritative) — it is never free-run under world gravity, so a settled
/// object the sim has gone silent about cannot drift.
pub(crate) fn drive_physical_objects(
    time: Res<Time>,
    liveness: Res<CircuitLiveness>,
    dilations: Res<RegionTimeDilation>,
    terrain: Res<TerrainState>,
    mut objects: Query<(
        Entity,
        Ref<PhysicalObject>,
        Option<&mut PhysicsInterp>,
        &mut Transform,
    )>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    let dt_raw = time.delta_secs();
    // The circuit looks stalled if no inbound event has been seen for longer than
    // the phase-out window (the analogue of `isBlocked` / a stale last-packet time).
    let circuit_stale = liveness
        .last_event_secs
        .is_some_and(|seen| now - seen > PHASE_OUT_START_SECS);

    for (entity, phys, interp, mut transform) in &mut objects {
        let Some(mut interp) = interp else {
            // Newly physical: attach the kinematic body + cuboid collider, seed the
            // interpolation, and place the entity at the authoritative pose.
            let [ex, ey, ez] = collider_extents(&phys.scale);
            debug!("physical object {entity} → kinematic body ({ex:.2}×{ey:.2}×{ez:.2} m)");
            place(
                &mut transform,
                &phys.position,
                &sl_rotation_to_quat(&phys.rotation),
            );
            commands.entity(entity).insert((
                RigidBody::Kinematic,
                Collider::cuboid(ex, ey, ez),
                PhysicsInterp::seeded(&phys, now),
            ));
            continue;
        };

        // A fresh server update: snap the prediction back to truth and restart. The
        // collider itself (including a rebuild on resize) is owned by
        // [`refine_physical_colliders`]; `reseed` only refreshes the cached scale.
        if phys.is_changed() {
            interp.reseed(&phys, now);
            place(&mut transform, &phys.position, &interp.motion.rotation);
            continue;
        }

        // Between updates: dead-reckon forward.
        let region = interp.motion.region_handle;
        let region_dilation = dilations.per_region.get(&region).copied().unwrap_or(1.0);
        let dt = clamp_dilation(region_dilation) * dt_raw;
        let time_since_last_update = now - interp.last_message_secs;
        if dt <= 0.0 || time_since_last_update <= 0.0 {
            interp.last_interp_secs = now;
            continue;
        }

        let phase_out = phase_out_factor(
            time_since_last_update,
            now - interp.last_interp_secs,
            interp.last_interp_secs - interp.last_message_secs > PHASE_OUT_START_SECS,
            circuit_stale,
        );
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "phase_out is a 0.0..=1.0 ratio; f32 precision is ample"
        )]
        let phase_out_f32 = phase_out as f32;
        let neighbours = neighbours_known(&dilations, region);
        let [scale_x, scale_y, scale_z] = interp.collider_scale;
        advance_motion(
            &mut interp.motion,
            neighbours,
            dt,
            phase_out_f32,
            now,
            // Objects use the permissive ground floor (they may sink underground);
            // only a laggy prediction running arbitrarily far below is stopped.
            |predicted_x, predicted_y| {
                ground_floor(
                    terrain.land_height(region, predicted_x, predicted_y),
                    &Vector {
                        x: scale_x,
                        y: scale_y,
                        z: scale_z,
                    },
                )
            },
        );
        interp.last_interp_secs = now;

        place(
            &mut transform,
            &array_to_vector(interp.motion.position),
            &interp.motion.rotation,
        );
    }
}

/// A `[f32; 3]` motion component triple as a Second Life [`Vector`], for handing a
/// predicted position back to [`place`] (destructured rather than indexed to satisfy
/// the workspace lints).
const fn array_to_vector(array: [f32; 3]) -> Vector {
    let [x, y, z] = array;
    Vector { x, y, z }
}

/// Write a physical object's Second Life region-local pose into its Bevy world
/// [`Transform`], applying the single Second Life → Bevy basis change (the same
/// mapping a root object's `object_transform` uses). The entity carries no scale
/// (it rides the geometry holder), so only translation and rotation are set.
fn place(transform: &mut Transform, position: &Vector, sl_rotation: &Quat) {
    transform.translation = sl_to_bevy_vec(position);
    transform.rotation = sl_to_bevy_rotation().mul_quat(*sl_rotation);
}

/// Strip the kinematic body from an entity that is no longer a physical root (its
/// [`PhysicalObject`] marker was removed by [`apply_physics`] — e.g. a prim made
/// non-physical, relinked as a child, or attached), so it stops being driven.
pub(crate) fn detach_physical_bodies(
    stale: Query<Entity, (With<PhysicsInterp>, Without<PhysicalObject>)>,
    mut commands: Commands,
) {
    for entity in &stale {
        commands
            .entity(entity)
            .remove::<(RigidBody, Collider, PhysicsInterp, RefinedCollider)>();
    }
}

/// The stricter `getMinAllowedZ` ground floor the reference viewer applies to an
/// **avatar**: the land height under it plus half its bounding-box height, so a
/// laggy avatar's reported (near-pelvis) position stays above the terrain and its
/// feet do not sink under it (`resolveLandHeightGlobal + 0.5 * size.mV[VZ]`). This
/// is the one guard [`ground_floor`] deliberately keeps permissive for objects
/// (which may legitimately sink underground). `None` land height (terrain not yet
/// ingested) means no floor.
fn avatar_ground_floor(land_height: Option<f32>, height: f32) -> Option<f32> {
    land_height.map(|land| land + 0.5 * height)
}

/// The authoritative kinematic motion of a full-object avatar (`pcode` 47) as of
/// its last `ObjectUpdate`, attached to the avatar's anchor entity by
/// [`apply_object`](crate::avatars) and change-detected: a fresh insert on every
/// update reseeds the interpolation. Its presence marks the avatar anchors
/// [`drive_avatar_motion`] dead-reckons between updates. Coarse (minimap-only)
/// avatars carry no velocity and so get no [`AvatarMotion`].
#[derive(Component, Clone)]
pub(crate) struct AvatarMotion {
    /// Region-local position (metres, Second Life Z-up frame).
    position: Vector,
    /// Linear velocity (metres/second).
    velocity: Vector,
    /// Linear acceleration (metres/second²).
    acceleration: Vector,
    /// Orientation (a Second Life unit quaternion).
    rotation: Rotation,
    /// Angular velocity (rotation axis scaled by radians/second).
    angular_velocity: Vector,
    /// The region this avatar lives in, for the region-edge / neighbour lookups.
    region_handle: RegionHandle,
    /// The avatar's bounding-box height (object scale Z), for the ground floor.
    height: f32,
    /// Whether the anchor applies the object's orientation (a rigged body root) or
    /// stays upright (a placeholder sphere, which does not visibly rotate).
    apply_rotation: bool,
}

impl AvatarMotion {
    /// The avatar's current heading (yaw about the Second Life up axis, radians),
    /// extracted from its reported orientation. The viewer's movement controls
    /// ([`crate::movement`]) seed the walk heading from this so the first step does
    /// not snap the avatar to an arbitrary facing.
    #[must_use]
    pub(crate) fn yaw(&self) -> f32 {
        let Rotation { x, y, z, s } = &self.rotation;
        // Yaw about Z from a unit quaternion (`atan2(2(sz + xy), 1 - 2(y² + z²))`).
        let siny_cosp = 2.0 * (s * z + x * y);
        let cosy_cosp = 1.0 - 2.0 * (y * y + z * z);
        siny_cosp.atan2(cosy_cosp)
    }

    /// The avatar's vertical (Second Life Z-up) velocity component (metres/second):
    /// positive climbing, negative descending / falling. The client-side locomotion
    /// fallback ([`crate::locomotion`]) reads this to pick the ascend / descend /
    /// fall states — the only states with no advertised control-flag intent.
    #[must_use]
    pub(crate) const fn vertical_speed(&self) -> f32 {
        self.velocity.z
    }

    /// Whether this avatar's authoritative position sits at (or within `margin`
    /// metres above) the **stricter avatar ground floor** ([`avatar_ground_floor`]:
    /// `land + 0.5 * height`) for the terrain beneath it — i.e. the avatar is on /
    /// very close to the ground rather than up in the air. The viewer's movement
    /// controls ([`crate::movement`]) use this to auto-stop flying on landing
    /// (P31.11). Returns `false` when the land height under the avatar is not yet
    /// known (terrain not ingested), so an unknown floor never forces a landing.
    #[must_use]
    pub(crate) fn at_ground_floor(&self, terrain: &TerrainState, margin: f32) -> bool {
        avatar_ground_floor(
            terrain.land_height(self.region_handle, self.position.x, self.position.y),
            self.height,
        )
        .is_some_and(|floor| self.position.z <= floor + margin)
    }

    /// The region this avatar is in — the frame the terrain queries and its reported
    /// position are expressed in.
    #[must_use]
    pub(crate) const fn region(&self) -> RegionHandle {
        self.region_handle
    }

    /// The avatar's reported linear velocity (Second Life Z-up metres/second, region
    /// frame). The walk-adjust foot-slip servo (P31.14) matches the walk animation's
    /// playback speed to this.
    #[must_use]
    pub(crate) const fn sl_velocity(&self) -> Vec3 {
        Vec3::new(self.velocity.x, self.velocity.y, self.velocity.z)
    }

    /// The avatar's reported angular velocity (rotation axis scaled by radians/second,
    /// region frame). The fly-adjust bank (P31.14) rolls the pelvis into a turn by its
    /// Z component, exactly as the reference's `LLFlyAdjustMotion` does.
    #[must_use]
    pub(crate) const fn sl_angular_velocity(&self) -> Vec3 {
        Vec3::new(
            self.angular_velocity.x,
            self.angular_velocity.y,
            self.angular_velocity.z,
        )
    }

    /// Build the authoritative motion from an avatar's object update. `apply_rotation`
    /// is `true` for a rigged body root (whose anchor carries the object rotation)
    /// and `false` for a placeholder sphere.
    #[must_use]
    pub(crate) fn from_object(object: &Object, apply_rotation: bool) -> Self {
        Self {
            position: object.motion.position.clone(),
            velocity: object.motion.velocity.clone(),
            acceleration: object.motion.acceleration.clone(),
            rotation: object.motion.rotation.clone(),
            angular_velocity: object.motion.angular_velocity.clone(),
            region_handle: object.region_handle,
            height: object.scale.z,
            apply_rotation,
        }
    }
}

/// The viewer-side interpolation state for one avatar, owned entirely by
/// [`drive_avatar_motion`]: the shared dead-reckoning prediction plus the avatar's
/// ground-floor height and whether its anchor carries the object rotation. Unlike
/// the object path, this driver moves the anchor by the *delta* between successive
/// predictions, so the pelvis / shoe vertical render offset (owned by
/// [`apply_object`](crate::avatars) and refreshed by the appearance path) is left
/// untouched.
#[derive(Component)]
pub(crate) struct AvatarInterp {
    /// The shared dead-reckoning prediction (pose + motion) advanced each frame.
    motion: MotionState,
    /// Elapsed seconds when the last server update was ingested.
    last_message_secs: f64,
    /// Elapsed seconds at the last interpolation step.
    last_interp_secs: f64,
    /// The avatar's bounding-box height, for the stricter ground floor.
    height: f32,
    /// Whether to write the predicted orientation onto the anchor (a rigged body).
    apply_rotation: bool,
    /// The orientation actually written to the anchor this frame (Bevy space), eased
    /// toward the authoritative / dead-reckoned facing each frame rather than snapped
    /// to it (P31.7). This decouples the rendered turn from the sparse authoritative
    /// rotation updates — the own avatar's facing arrives only as terse
    /// `ObjectUpdate`s echoing the client-driven `SetRotation` (P31.5), so without
    /// this easing a turn snaps between those updates while translation stays smooth.
    rendered_rotation: Quat,
}

impl AvatarInterp {
    /// Seed the interpolation state from an authoritative update at time `now`.
    fn seeded(motion: &AvatarMotion, now: f64) -> Self {
        let motion_state = MotionState::new(
            &motion.position,
            &motion.velocity,
            &motion.acceleration,
            &motion.rotation,
            &motion.angular_velocity,
            motion.region_handle,
        );
        // Start the eased orientation at the authoritative facing so the avatar does
        // not visibly rotate into place from identity on its first frame.
        let rendered_rotation = bevy_rotation_of(&motion_state);
        Self {
            motion: motion_state,
            last_message_secs: now,
            last_interp_secs: now,
            height: motion.height,
            apply_rotation: motion.apply_rotation,
            rendered_rotation,
        }
    }

    /// Re-seed the predicted pose to a fresh authoritative update at time `now`,
    /// snapping the prediction back to the server truth and restarting the timers.
    fn reseed(&mut self, motion: &AvatarMotion, now: f64) {
        self.motion = MotionState::new(
            &motion.position,
            &motion.velocity,
            &motion.acceleration,
            &motion.rotation,
            &motion.angular_velocity,
            motion.region_handle,
        );
        self.last_message_secs = now;
        self.last_interp_secs = now;
        self.height = motion.height;
        self.apply_rotation = motion.apply_rotation;
    }
}

/// Dead-reckon every full-object avatar between server updates (P31.4), the avatar
/// counterpart of [`drive_physical_objects`]: on the frame an [`AvatarMotion`]
/// update lands, [`apply_object`](crate::avatars) has already snapped the anchor to
/// the authoritative pose, so this only (re)seeds the interpolation; between
/// updates it advances the predicted pose with the same phase-out taper and
/// geometric clamps as the object path — but with the **stricter avatar ground
/// floor** ([`avatar_ground_floor`]) so a laggy avatar does not sink under the
/// terrain. The avatar stays kinematic (sim-authoritative); the predicted motion is
/// applied to the anchor as a translation *delta* (plus, for a rigged body, the
/// predicted orientation), leaving the pelvis / shoe render offset intact.
pub(crate) fn drive_avatar_motion(
    time: Res<Time>,
    liveness: Res<CircuitLiveness>,
    dilations: Res<RegionTimeDilation>,
    terrain: Res<TerrainState>,
    mut avatars: Query<(
        Entity,
        Ref<AvatarMotion>,
        Option<&mut AvatarInterp>,
        &mut Transform,
    )>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    let dt_raw = time.delta_secs();
    let circuit_stale = liveness
        .last_event_secs
        .is_some_and(|seen| now - seen > PHASE_OUT_START_SECS);

    for (entity, motion, interp, mut transform) in &mut avatars {
        let Some(mut interp) = interp else {
            // Newly tracked: seed the interpolation. The anchor is already at the
            // authoritative pose (placed by `apply_object`), so nothing to move.
            debug!(
                "avatar {entity} → dead-reckoned (height {:.2} m, rotates {})",
                motion.height, motion.apply_rotation
            );
            commands
                .entity(entity)
                .insert(AvatarInterp::seeded(&motion, now));
            continue;
        };

        // A fresh server update: `apply_object` already snapped the anchor's
        // translation to truth, so just reseed the prediction and restart the timers.
        // The orientation, though, is *not* snapped — it eases toward the new facing
        // (P31.7), so a client-driven turn stays fluid across sparse rotation updates.
        if motion.is_changed() {
            interp.reseed(&motion, now);
            apply_smoothed_rotation(&mut interp, &mut transform, dt_raw);
            continue;
        }

        // Between updates: dead-reckon forward.
        let region = interp.motion.region_handle;
        let region_dilation = dilations.per_region.get(&region).copied().unwrap_or(1.0);
        let dt = clamp_dilation(region_dilation) * dt_raw;
        let time_since_last_update = now - interp.last_message_secs;
        if dt <= 0.0 || time_since_last_update <= 0.0 {
            interp.last_interp_secs = now;
            apply_smoothed_rotation(&mut interp, &mut transform, dt_raw);
            continue;
        }

        let phase_out = phase_out_factor(
            time_since_last_update,
            now - interp.last_interp_secs,
            interp.last_interp_secs - interp.last_message_secs > PHASE_OUT_START_SECS,
            circuit_stale,
        );
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "phase_out is a 0.0..=1.0 ratio; f32 precision is ample"
        )]
        let phase_out_f32 = phase_out as f32;
        let neighbours = neighbours_known(&dilations, region);
        let height = interp.height;
        let previous = interp.motion.position;
        advance_motion(
            &mut interp.motion,
            neighbours,
            dt,
            phase_out_f32,
            now,
            // Avatars use the stricter ground floor so a laggy avatar does not sink
            // under the terrain (the one guard the object path leaves permissive).
            |predicted_x, predicted_y| {
                avatar_ground_floor(
                    terrain.land_height(region, predicted_x, predicted_y),
                    height,
                )
            },
        );
        interp.last_interp_secs = now;

        // Apply the Second Life-space position change as a Bevy translation delta
        // (the basis change is linear, so the delta converts directly), so the
        // pelvis / shoe vertical render offset baked into the anchor is preserved.
        let [prev_x, prev_y, prev_z] = previous;
        let [next_x, next_y, next_z] = interp.motion.position;
        let delta = sl_to_bevy_vec(&Vector {
            x: next_x - prev_x,
            y: next_y - prev_y,
            z: next_z - prev_z,
        });
        transform.translation = Vec3::new(
            transform.translation.x + delta.x,
            transform.translation.y + delta.y,
            transform.translation.z + delta.z,
        );
        // Ease (not snap) the rendered facing toward the dead-reckoned orientation,
        // the rotation counterpart of the smoothed translation above (P31.7).
        apply_smoothed_rotation(&mut interp, &mut transform, dt_raw);
    }
}

/// The per-object physics-shape data the viewer has learned, keyed by full
/// [`ObjectKey`] (the id the `GetObjectPhysicsData` capability reply uses). Folded
/// by [`ingest_object_physics_data`] from both the capability reply
/// ([`SlSessionEvent::ObjectPhysicsData`]) and the unsolicited event-queue push
/// ([`SlSessionEvent::ObjectPhysicsProperties`]), and read by
/// [`refine_physical_colliders`] to pick each physical object's collision shape.
#[derive(Resource, Default)]
pub(crate) struct ObjectPhysicsShapes {
    /// The latest physics data for each object, keyed by full key.
    data: HashMap<ObjectKey, ObjectPhysicsData>,
    /// The objects a `GetObjectPhysicsData` request has already been sent for, so
    /// [`request_object_physics_data`] asks the grid exactly once per object.
    requested: HashSet<ObjectKey>,
}

/// Request the `GetObjectPhysicsData` capability data for every newly-flagged
/// physical object exactly once. The grid only *pushes* `ObjectPhysicsProperties`
/// when a prim's physics material changes (OpenSim `SceneGraph.UpdateExtraPhysics`),
/// so a proactive request is the reliable way to learn a streamed-in object's
/// collision shape. A no-op when the region seed omits the capability.
pub(crate) fn request_object_physics_data(
    new_physical: Query<&PhysicalObject, Added<PhysicalObject>>,
    mut shapes: ResMut<ObjectPhysicsShapes>,
    mut writer: MessageWriter<SlCommand>,
) {
    let mut object_ids = Vec::new();
    for phys in &new_physical {
        if shapes.requested.insert(phys.full_key) {
            object_ids.push(phys.full_key);
        }
    }
    if !object_ids.is_empty() {
        writer.write(SlCommand(Command::RequestObjectPhysicsData { object_ids }));
    }
}

/// Fold the object physics data from both delivery paths into
/// [`ObjectPhysicsShapes`]: the `GetObjectPhysicsData` capability reply (already
/// keyed by full [`ObjectKey`]) and the unsolicited `ObjectPhysicsProperties`
/// event-queue push (keyed by [`ScopedObjectId`](sl_client_bevy::ScopedObjectId),
/// translated to the full key via the tracked object table so both paths land
/// under the same key).
pub(crate) fn ingest_object_physics_data(
    mut events: MessageReader<SlEvent>,
    objects: Res<ObjectState>,
    mut shapes: ResMut<ObjectPhysicsShapes>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectPhysicsData(entries) => {
                for (key, data) in entries {
                    shapes.data.insert(*key, *data);
                }
            }
            SlSessionEvent::ObjectPhysicsProperties(entries) => {
                for (scoped, data) in entries {
                    if let Some(key) = objects.full_key(scoped) {
                        shapes.data.insert(key, *data);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Records which collision shape (and at what scale) the current avian [`Collider`]
/// on a physical object was built for, so [`refine_physical_colliders`] rebuilds it
/// only when the shape data, the geometry, or the scale actually change.
#[derive(Component)]
pub(crate) struct RefinedCollider {
    /// The physics-shape type the collider was built for, or [`None`] while the
    /// object's physics data has not yet arrived (a placeholder cuboid stands in).
    shape: Option<PhysicsShapeType>,
    /// Whether the collider is the real geometry-derived shape (`true`) or a
    /// stand-in cuboid awaiting the shape data / the object's tessellated geometry
    /// (`false`) — the latter is retried each frame until the geometry is ready.
    from_geometry: bool,
    /// The object scale (floored extents, metres per axis) the collider was built
    /// for, so a genuine resize rebuilds it.
    scale: [f32; 3],
}

/// Whether a physics-shape type needs the object's tessellated geometry to build
/// its collider (convex hull / prim / an unrecognised type), as opposed to
/// [`PhysicsShapeType::None`] (no collider) which needs no geometry.
const fn shape_wants_geometry(shape: PhysicsShapeType) -> bool {
    matches!(
        shape,
        PhysicsShapeType::Prim | PhysicsShapeType::ConvexHull | PhysicsShapeType::Other(_)
    )
}

/// Whether two floored collider-extent triples differ enough to warrant a rebuild.
fn extents_differ(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(&b).any(|(x, y)| (x - y).abs() > f32::EPSILON)
}

/// Append a mesh's triangle indices to `out`, offsetting each vertex index by
/// `base` (the count of vertices already gathered from earlier faces) so several
/// faces combine into one trimesh index buffer. Handles both `u16` and `u32` index
/// buffers; a non-triangle-list remainder is ignored.
fn append_triangles(out: &mut Vec<[u32; 3]>, indices: &Indices, base: u32) {
    match indices {
        Indices::U16(values) => {
            for tri in values.chunks_exact(3) {
                if let [a, b, c] = tri {
                    out.push([
                        base.saturating_add(u32::from(*a)),
                        base.saturating_add(u32::from(*b)),
                        base.saturating_add(u32::from(*c)),
                    ]);
                }
            }
        }
        Indices::U32(values) => {
            for tri in values.chunks_exact(3) {
                if let [a, b, c] = tri {
                    out.push([
                        base.saturating_add(*a),
                        base.saturating_add(*b),
                        base.saturating_add(*c),
                    ]);
                }
            }
        }
    }
}

/// Gather a physical object's own tessellated geometry — the faces under its
/// [`GeometryHolder`] child, **excluding** the linkset child prims that also parent
/// to the object entity — as a point cloud plus a triangle index buffer, each
/// vertex scaled by the object scale into the object entity's local frame (the
/// frame its avian [`Collider`] lives in, matching how the same faces render
/// through the geometry holder's scale). Empty until the geometry has been spawned
/// and its meshes uploaded (an object still waiting on a mesh / sculpt fetch).
fn gather_object_geometry(
    object_entity: Entity,
    scale: [f32; 3],
    children_q: &Query<&Children>,
    holders: &Query<(), With<GeometryHolder>>,
    mesh_handles: &Query<&Mesh3d>,
    meshes: &Assets<Mesh>,
) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let mut points = Vec::new();
    let mut indices = Vec::new();
    let [sx, sy, sz] = scale;
    let Ok(object_children) = children_q.get(object_entity) else {
        return (points, indices);
    };
    // The object's own geometry hangs off its single geometry-holder child; its
    // linkset children (separate `SceneObject`s with their own holders/scales) are
    // skipped so a root prim's collider is its own shape, not the whole linkset.
    let Some(holder) = object_children
        .iter()
        .find(|&child| holders.get(child).is_ok())
    else {
        return (points, indices);
    };
    let Ok(faces) = children_q.get(holder) else {
        return (points, indices);
    };
    for &face in faces {
        let Ok(mesh3d) = mesh_handles.get(face) else {
            continue;
        };
        let Some(mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let base = u32::try_from(points.len()).unwrap_or(u32::MAX);
        for position in positions {
            let [x, y, z] = *position;
            points.push(Vec3::new(x * sx, y * sy, z * sz));
        }
        if let Some(mesh_indices) = mesh.indices() {
            append_triangles(&mut indices, mesh_indices, base);
        }
    }
    (points, indices)
}

/// Replace the P31.2 placeholder cuboid on each physical object with a collider
/// that matches its simulator `LLPhysicsShapeType` and geometry, once both the
/// physics-shape data ([`ObjectPhysicsShapes`]) and the object's tessellated
/// geometry are available:
///
/// - **unknown** (data not yet in) → keep the placeholder cuboid;
/// - **none** ([`PhysicsShapeType::None`]) → no collider (a physical prim that
///   collides with nothing);
/// - **convex hull** → [`Collider::convex_hull`] of the prim / mesh vertices;
/// - **prim** (or an unrecognised type) → a [`Collider::trimesh`] of that geometry.
///
/// The result is recorded in [`RefinedCollider`] so a collider is rebuilt only on a
/// real change (new shape data, a resize, or geometry finally arriving). These
/// colliders are inert on the kinematic movers themselves — they matter once Phases
/// 32 / 34 add dynamic bodies that collide against them.
pub(crate) fn refine_physical_colliders(
    shapes: Res<ObjectPhysicsShapes>,
    objects: Query<(Entity, &PhysicalObject, Option<&RefinedCollider>), With<PhysicsInterp>>,
    children_q: Query<&Children>,
    holders: Query<(), With<GeometryHolder>>,
    mesh_handles: Query<&Mesh3d>,
    meshes: Res<Assets<Mesh>>,
    mut commands: Commands,
) {
    for (entity, phys, existing) in &objects {
        let desired = shapes
            .data
            .get(&phys.full_key)
            .map(|data| data.physics_shape_type);
        let scale = collider_extents(&phys.scale);
        let scale_changed = existing.is_none_or(|state| extents_differ(state.scale, scale));
        let shape_changed = existing.is_none_or(|state| state.shape != desired);
        // A geometry-needing shape whose collider is still the placeholder cuboid:
        // retry the geometry gather each frame until the meshes are uploaded.
        let geometry_pending = existing.is_some_and(|state| !state.from_geometry)
            && desired.is_some_and(shape_wants_geometry);
        if !(scale_changed || shape_changed || geometry_pending) {
            continue;
        }
        let [ex, ey, ez] = scale;

        match desired {
            // Physics data not yet learned: keep the P31.2 placeholder cuboid, sized
            // to the current scale, until the shape type arrives.
            None => {
                commands.entity(entity).insert((
                    Collider::cuboid(ex, ey, ez),
                    RefinedCollider {
                        shape: None,
                        from_geometry: false,
                        scale,
                    },
                ));
            }
            // No collision shape: strip any collider, leaving the kinematic body.
            Some(PhysicsShapeType::None) => {
                debug!("physical object {entity} → no collider (PhysicsShapeType::None)");
                commands.entity(entity).remove::<Collider>();
                commands.entity(entity).insert(RefinedCollider {
                    shape: desired,
                    from_geometry: true,
                    scale,
                });
            }
            // Convex hull / exact prim / an unrecognised type: build from the
            // object's own tessellated geometry.
            Some(shape) => {
                let (points, indices) = gather_object_geometry(
                    entity,
                    scale,
                    &children_q,
                    &holders,
                    &mesh_handles,
                    &meshes,
                );
                if points.is_empty() {
                    // Geometry not spawned / uploaded yet: keep a placeholder cuboid
                    // (installed only on a real change, not on a pure retry) and try
                    // again next frame.
                    if scale_changed || shape_changed {
                        commands.entity(entity).insert(Collider::cuboid(ex, ey, ez));
                    }
                    commands.entity(entity).insert(RefinedCollider {
                        shape: desired,
                        from_geometry: false,
                        scale,
                    });
                    continue;
                }
                let point_count = points.len();
                let collider = match shape {
                    PhysicsShapeType::ConvexHull => Collider::convex_hull(points)
                        .unwrap_or_else(|| Collider::cuboid(ex, ey, ez)),
                    // Prim (and any unrecognised type) uses the exact geometry.
                    _ => Collider::trimesh(points, indices),
                };
                debug!("physical object {entity} → {shape:?} collider from {point_count} vertices");
                commands.entity(entity).insert((
                    collider,
                    RefinedCollider {
                        shape: desired,
                        from_geometry: true,
                        scale,
                    },
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClampInput, MAX_INTERP_SECS, MotionState, PHASE_OUT_START_SECS, REGION_MAX_HEIGHT_M,
        REGION_WIDTH_M, ROTATION_SMOOTHING_TAU_SECS, SL_GRAVITY_Z, advance_motion, angular_step,
        append_triangles, avatar_ground_floor, clamp_dilation, clamp_prediction, dead_reckon,
        extents_differ, ground_floor, neighbours_known, phase_out_factor, rotation_smoothing_alpha,
        shape_wants_geometry, sl_gravity,
    };
    use crate::physics::RegionTimeDilation;
    use avian3d::prelude::{Collider, SimpleCollider as _};
    use bevy::math::{Quat, Vec3};
    use bevy::mesh::Indices;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{PhysicsShapeType, RegionHandle, Rotation, Vector};

    /// Assert two `f32` are equal within a tight tolerance (the workspace lints
    /// forbid a strict `float_cmp`, and the clamp results are exact anyway).
    fn approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= f32::EPSILON,
            "{actual} should equal {expected}"
        );
    }

    /// Assert two `f32` are equal within a looser tolerance for accumulated
    /// floating-point arithmetic.
    fn near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1.0e-4,
            "{actual} should be about {expected}"
        );
    }

    /// Component-wise [`near`] for a 3-vector (the workspace lints forbid a strict
    /// float-array equality).
    fn near3(actual: [f32; 3], expected: [f32; 3]) {
        for (a, e) in actual.iter().zip(&expected) {
            near(*a, *e);
        }
    }

    /// Second Life's Z-up gravity maps to a Bevy `-Y` vector of the same
    /// magnitude — the physics world falls "down" in the rendered scene.
    #[test]
    fn gravity_maps_z_up_to_bevy_down() {
        // Straight down in Bevy's Y-up frame at 9.8 m/s².
        assert!(sl_gravity().abs_diff_eq(Vec3::new(0.0, SL_GRAVITY_Z, 0.0), 1.0e-6));
        assert!(sl_gravity().abs_diff_eq(Vec3::new(0.0, -9.8, 0.0), 1.0e-6));
    }

    /// A healthy region (dilation `1.0`) runs physics at full speed; a laden one
    /// scales it down; the endpoints of the wire domain pass through unchanged.
    #[test]
    fn dilation_clamps_into_the_relative_speed_domain() {
        approx(clamp_dilation(1.0), 1.0);
        approx(clamp_dilation(0.5), 0.5);
        approx(clamp_dilation(0.0), 0.0);
    }

    /// An out-of-range or non-finite dilation can never poison the physics clock
    /// (avian's `set_relative_speed` would panic on a negative / non-finite
    /// ratio): it is clamped into range, and a `NaN` falls back to full speed.
    #[test]
    fn dilation_guards_against_bad_values() {
        approx(clamp_dilation(-0.5), 0.0);
        approx(clamp_dilation(2.0), 1.0);
        approx(clamp_dilation(f32::NAN), 1.0);
        approx(clamp_dilation(f32::INFINITY), 1.0);
    }

    /// Dead-reckoning at full phase-out advances position by the reference
    /// viewer's `(vel + 0.5*(dt - PHYSICS_TIMESTEP)*accel) * dt` and velocity by
    /// `accel * dt`. With `dt == PHYSICS_TIMESTEP` the acceleration correction
    /// vanishes, so the position step is exactly `vel * dt`.
    #[test]
    fn dead_reckon_matches_reference_formula() {
        let dt = 1.0 / 45.0;
        let (position, velocity) =
            dead_reckon([0.0, 0.0, 10.0], [2.0, 0.0, 0.0], [0.0, 0.0, -9.8], dt, 1.0);
        let [px, _, pz] = position;
        near(px, 2.0 * dt);
        // The Z position step is `vel_z * dt` (the accel term is zero at this dt).
        near(pz, 10.0);
        let [_, _, vz] = velocity;
        near(vz, -9.8 * dt);
    }

    /// A zero phase-out freezes the object: no position or velocity change.
    #[test]
    fn dead_reckon_phase_out_zero_freezes() {
        let (position, velocity) =
            dead_reckon([5.0, 6.0, 7.0], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0], 0.1, 0.0);
        near3(position, [5.0, 6.0, 7.0]);
        near3(velocity, [1.0, 1.0, 1.0]);
    }

    /// The phase-out stays at full strength until the circuit looks stalled and
    /// the silence exceeds the start threshold, then ramps `1.0 → 0.0` between the
    /// start and max windows, reaching zero past the max.
    #[test]
    fn phase_out_ramps_only_when_stalled() {
        // A healthy circuit never tapers, however long the object is silent.
        assert!((phase_out_factor(10.0, 0.0, false, false) - 1.0).abs() < 1.0e-9);
        // Stalled but still inside the start window: full strength.
        assert!((phase_out_factor(1.0, 0.0, false, true) - 1.0).abs() < 1.0e-9);
        // Halfway between start (2 s) and max (3 s): half strength.
        let mid = 0.5 * (PHASE_OUT_START_SECS + MAX_INTERP_SECS);
        assert!((phase_out_factor(mid, 0.0, false, true) - 0.5).abs() < 1.0e-9);
        // Past the max window: fully stopped.
        assert!(phase_out_factor(MAX_INTERP_SECS + 1.0, 0.0, false, true).abs() < 1.0e-9);
    }

    /// A spin about Z advances the orientation by `omega * dt` radians; a zero
    /// angular velocity leaves it untouched.
    #[test]
    fn angular_step_rotates_about_axis() {
        let quarter = core::f32::consts::FRAC_PI_2;
        let rotated = angular_step(Quat::IDENTITY, [0.0, 0.0, quarter], 1.0);
        let expected = Quat::from_rotation_z(quarter);
        assert!(rotated.abs_diff_eq(expected, 1.0e-5) || rotated.abs_diff_eq(-expected, 1.0e-5));
        let still = angular_step(Quat::IDENTITY, [0.0, 0.0, 0.0], 1.0);
        assert!(still.abs_diff_eq(Quat::IDENTITY, 1.0e-6));
    }

    /// The height clamps: an object predicted above the region ceiling is capped
    /// to it, and one predicted below the ground floor is lifted to it.
    #[test]
    fn clamp_prediction_bounds_height() {
        let ceilinged = clamp_prediction(ClampInput {
            position: [100.0, 100.0, REGION_MAX_HEIGHT_M + 500.0],
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            floor: None,
            neighbours: [true; 4],
            region_cross_expire: None,
            now: 0.0,
        });
        let [_, _, z] = ceilinged.position;
        near(z, REGION_MAX_HEIGHT_M);

        let floored = clamp_prediction(ClampInput {
            position: [100.0, 100.0, -50.0],
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            floor: Some(20.0),
            neighbours: [true; 4],
            region_cross_expire: None,
            now: 0.0,
        });
        let [_, _, z] = floored.position;
        near(z, 20.0);
    }

    /// Leaving the region into a **void** (no neighbour) clips the position to the
    /// edge and zeroes velocity + acceleration — the object waits for a server
    /// update instead of dead-reckoning off into infinity.
    #[test]
    fn clamp_prediction_clips_at_empty_edge() {
        let out = clamp_prediction(ClampInput {
            position: [-5.0, 100.0, 30.0],
            velocity: [-3.0, 0.0, 0.0],
            acceleration: [0.0, 0.0, -9.8],
            floor: None,
            neighbours: [false, false, false, false],
            region_cross_expire: None,
            now: 0.0,
        });
        let [x, _, _] = out.position;
        near(x, 0.0);
        near3(out.velocity, [0.0; 3]);
        near3(out.acceleration, [0.0; 3]);
    }

    /// Leaving into a **known neighbour** is a border crossing: the position is
    /// left beyond the edge (it continues into the neighbour), acceleration is
    /// zeroed, and a crossing deadline is opened; past the deadline motion stops.
    #[test]
    fn clamp_prediction_bounds_region_crossing() {
        let entering = clamp_prediction(ClampInput {
            position: [REGION_WIDTH_M + 5.0, 100.0, 30.0],
            velocity: [3.0, 0.0, 0.0],
            acceleration: [0.0, 0.0, -9.8],
            floor: None,
            neighbours: [false, true, false, false],
            region_cross_expire: None,
            now: 10.0,
        });
        let [x, _, _] = entering.position;
        near(x, REGION_WIDTH_M + 5.0);
        near3(entering.acceleration, [0.0; 3]);
        assert!(entering.region_cross_expire.is_some());

        let expired = clamp_prediction(ClampInput {
            position: [REGION_WIDTH_M + 5.0, 100.0, 30.0],
            velocity: [3.0, 0.0, 0.0],
            acceleration: [0.0, 0.0, 0.0],
            floor: None,
            neighbours: [false, true, false, false],
            region_cross_expire: Some(10.5),
            now: 12.0,
        });
        near3(expired.velocity, [0.0; 3]);
        assert!(expired.region_cross_expire.is_none());
    }

    /// The ground floor is the land height minus the object's bounding radius
    /// (half its scale length), and is absent when no land height is known.
    #[test]
    fn ground_floor_subtracts_bounding_radius() {
        let scale = Vector {
            x: 2.0,
            y: 0.0,
            z: 0.0,
        };
        // radius = 0.5 * |(2,0,0)| = 1.0, so floor = 25.0 - 1.0.
        let floor = ground_floor(Some(25.0), &scale);
        assert!(
            floor.is_some_and(|f| (f - 24.0).abs() <= 1.0e-4),
            "floor should be about 24.0, got {floor:?}"
        );
        assert!(
            ground_floor(None, &scale).is_none(),
            "no floor without a known land height"
        );
    }

    /// Only the neighbour regions the session has actually heard from count as
    /// known — the analogue of the reference viewer's `clipToVisibleRegions`.
    #[test]
    fn neighbours_known_reads_seen_regions() {
        let width = 256_u32;
        let home = RegionHandle::from_global(1000 * width, 1000 * width);
        let east = RegionHandle::from_global(1001 * width, 1000 * width);
        let mut dilations = RegionTimeDilation::default();
        dilations.per_region.insert(home, 1.0);
        dilations.per_region.insert(east, 1.0);
        // `[-x, +x, -y, +y]`: only the eastern (+x) neighbour is known.
        assert_eq!(
            neighbours_known(&dilations, home),
            [false, true, false, false]
        );
    }

    /// Convex hull and prim shapes need the object geometry to build a collider;
    /// the "no shape" type needs none.
    #[test]
    fn shape_geometry_requirements() {
        assert!(shape_wants_geometry(PhysicsShapeType::Prim));
        assert!(shape_wants_geometry(PhysicsShapeType::ConvexHull));
        assert!(shape_wants_geometry(PhysicsShapeType::Other(7)));
        assert!(!shape_wants_geometry(PhysicsShapeType::None));
    }

    /// A resize past the float tolerance forces a collider rebuild; an unchanged
    /// scale does not.
    #[test]
    fn extents_differ_detects_a_resize() {
        assert!(!extents_differ([1.0, 2.0, 3.0], [1.0, 2.0, 3.0]));
        assert!(extents_differ([1.0, 2.0, 3.0], [1.0, 2.5, 3.0]));
    }

    /// Combining several faces into one trimesh index buffer offsets each face's
    /// indices by the running vertex count, and both `u16` and `u32` index buffers
    /// are handled.
    #[test]
    fn append_triangles_offsets_indices() {
        let mut out = Vec::new();
        // First face: three vertices at base 0.
        append_triangles(&mut out, &Indices::U16(vec![0, 1, 2]), 0);
        // Second face: its own 0/1/2 shifted past the first face's three vertices.
        append_triangles(&mut out, &Indices::U32(vec![0, 1, 2]), 3);
        assert_eq!(out, vec![[0, 1, 2], [3, 4, 5]]);
    }

    /// A convex hull can be built from the eight corners of a unit cube (the
    /// convex-hull physics-shape path), yielding a valid collider.
    #[test]
    fn convex_hull_from_cube_corners_builds() {
        let corners = vec![
            Vec3::new(-0.5, -0.5, -0.5),
            Vec3::new(0.5, -0.5, -0.5),
            Vec3::new(-0.5, 0.5, -0.5),
            Vec3::new(0.5, 0.5, -0.5),
            Vec3::new(-0.5, -0.5, 0.5),
            Vec3::new(0.5, -0.5, 0.5),
            Vec3::new(-0.5, 0.5, 0.5),
            Vec3::new(0.5, 0.5, 0.5),
        ];
        assert!(
            Collider::convex_hull(corners).is_some(),
            "eight cube corners should form a convex hull"
        );
    }

    /// A trimesh collider can be built from a two-triangle quad (the exact-prim
    /// physics-shape path) — the aabb spans the quad's extent.
    #[test]
    fn trimesh_from_quad_builds() {
        let vertices = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
        ];
        let collider = Collider::trimesh(vertices, vec![[0, 1, 2], [0, 2, 3]]);
        let aabb = collider.aabb(Vec3::ZERO, Quat::IDENTITY);
        assert!(
            (aabb.max.x - 2.0).abs() < 1.0e-4 && (aabb.max.y - 2.0).abs() < 1.0e-4,
            "trimesh aabb should span the 2x2 quad, got {aabb:?}"
        );
    }

    /// The avatar ground floor is the land height plus half the avatar's height —
    /// stricter than the object floor (which *subtracts* the radius) so the avatar's
    /// near-pelvis position stays above the terrain — and is absent without a land
    /// height.
    #[test]
    fn avatar_ground_floor_lifts_above_terrain() {
        // land 20 + 0.5 * height 2 = 21.
        let floor = avatar_ground_floor(Some(20.0), 2.0);
        assert!(
            floor.is_some_and(|f| (f - 21.0).abs() <= 1.0e-4),
            "avatar floor should be about 21.0, got {floor:?}"
        );
        assert!(
            avatar_ground_floor(None, 2.0).is_none(),
            "no floor without a known land height"
        );
    }

    /// A zero-length rotation. The identity Second Life quaternion, for seeding a
    /// [`MotionState`] whose orientation should stay put.
    fn identity_rotation() -> Rotation {
        Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        }
    }

    /// A falling body advances its position by the reference dead-reckoning step,
    /// and the supplied (avatar) ground floor lifts a prediction that drops below the
    /// terrain — the same `advance_motion` step drives both the object and avatar
    /// paths, differing only in that floor.
    #[test]
    fn advance_motion_dead_reckons_and_floors() {
        let vel = Vector {
            x: 2.0,
            y: 0.0,
            z: 0.0,
        };
        let accel = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let mut motion = MotionState::new(
            &Vector {
                x: 10.0,
                y: 10.0,
                z: 30.0,
            },
            &vel,
            &accel,
            &identity_rotation(),
            &Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            RegionHandle::from_global(1000 * 256, 1000 * 256),
        );
        // One second at full phase-out, all neighbours known (no edge clip). The
        // floor closure lifts the body to a high floor to prove the clamp runs.
        advance_motion(&mut motion, [true; 4], 1.0, 1.0, 0.0, |_x, _y| Some(100.0));
        let [x, _y, z] = motion.position;
        near(x, 12.0);
        near(z, 100.0);
    }

    /// A stationary avatar (zero velocity and acceleration) does not dead-reckon its
    /// position, however long it is silent — only a moving body extrapolates.
    #[test]
    fn advance_motion_leaves_a_still_body_put() {
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let mut motion = MotionState::new(
            &Vector {
                x: 5.0,
                y: 6.0,
                z: 7.0,
            },
            &zero,
            &zero,
            &identity_rotation(),
            &zero,
            RegionHandle::from_global(1000 * 256, 1000 * 256),
        );
        advance_motion(&mut motion, [true; 4], 1.0, 1.0, 0.0, |_x, _y| None);
        near3(motion.position, [5.0, 6.0, 7.0]);
    }

    /// The rotation-smoothing blend (P31.7) is `0` for a zero frame and rises toward
    /// `1` with the frame length, reaching ~63 % at exactly one time constant — the
    /// framerate-independent easing that turns sparse facing updates into a fluid
    /// turn. A non-positive frame snaps (blend `1`) so a paused frame cannot stall.
    #[test]
    fn rotation_smoothing_alpha_eases_by_frame_time() {
        near(rotation_smoothing_alpha(0.0), 1.0);
        near(rotation_smoothing_alpha(-1.0), 1.0);
        // One time constant covers 1 - 1/e ≈ 63.2 %.
        near(
            rotation_smoothing_alpha(ROTATION_SMOOTHING_TAU_SECS),
            1.0 - core::f32::consts::E.recip(),
        );
        // A longer frame eases further, but never past a full snap.
        let short = rotation_smoothing_alpha(0.008);
        let long = rotation_smoothing_alpha(0.033);
        assert!(short > 0.0 && short < long && long < 1.0);
    }

    /// Slerping the rendered facing toward a turned target by the per-frame blend
    /// advances part-way each frame (never snapping) and converges to the target once
    /// it stops moving — the whole point of P31.7. Yaw about the up axis stands in for
    /// the turning avatar.
    #[test]
    fn rotation_smoothing_converges_without_snapping() {
        let target = Quat::from_rotation_y(core::f32::consts::FRAC_PI_2);
        let mut rendered = Quat::IDENTITY;
        let alpha = rotation_smoothing_alpha(0.016);
        // The first frame closes part of the gap but does not reach the target.
        rendered = rendered.slerp(target, alpha);
        let after_one = rendered.angle_between(target);
        assert!(after_one > 0.0 && after_one < core::f32::consts::FRAC_PI_2);
        // Held steady, successive frames converge onto the target.
        for _ in 0..200 {
            rendered = rendered.slerp(target, alpha);
        }
        assert!(rendered.angle_between(target) < 1.0e-3);
    }
}
