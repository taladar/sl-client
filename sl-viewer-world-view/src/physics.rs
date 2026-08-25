//! The client-side physics foundation (P31.1): server-authoritative prim /
//! avatar dead-reckoning plus collision geometry for the viewer's spatial
//! queries, bridged into the Bevy Y-up scene.
//!
//! The viewer has **no dynamic solver** — it does not simulate physics. Every
//! moving object is a *kinematic mover* snapped to the server and dead-reckoned
//! between updates; the only client-side soft simulation (flexi prims, Phase 32)
//! runs its own bespoke chain solver. So this module needs no physics engine.
//! What it needs is (a) the dead-reckoning motion model and (b) collision
//! geometry for raycasts (camera collision) and prim–prim contact (collision
//! sounds). Both are provided directly: collision shapes are built with
//! [`parry3d`] and handed to the custom [`crate::raycast_index`] index (the
//! replacement for avian's `SpatialQuery`,
//! [[viewer-perf-custom-static-raycast-index]]), which maintains a BVH over the
//! static prims off-thread and a small linear set for the moving ones.
//!
//! - **Time dilation.** A laden region does not keep up with 45 Hz; it reports
//!   the fraction of real time its physics frame is achieving in the
//!   `RegionData.TimeDilation` field of every object-update message (surfaced as
//!   [`SlSessionEvent::TimeDilation`]) and folded into `RegionTimeDilation` by
//!   `ingest_time_dilation`. `drive_physical_objects` scales its
//!   dead-reckoning step by the agent region's dilation so the prediction slows
//!   in lock-step with the dilated sim instead of drifting ahead of it.
//!
//! **P31.2 — physical objects.** Every server-flagged physical root prim
//! (`FLAGS_USE_PHYSICS`, marked by `apply_physics` from
//! `apply_object`, both in [`sl_viewer_world_objects::objects`]) is a kinematic mover. The simulator stays
//! authoritative: `drive_physical_objects` snaps the pose to each
//! `ObjectUpdate` and, between updates, dead-reckons it forward exactly as the
//! reference viewer's `LLViewerObject::interpolateLinearMotion` does — the
//! velocity/acceleration extrapolation, the circuit-health phase-out (easing a
//! silent object to a halt once the circuit looks stalled), and the geometric
//! clamps (region-height ceiling, permissive ground floor, off-region-edge clip,
//! region-crossing cap). There is no free-run under gravity, so a settled object
//! the sim has gone silent about cannot drift.
//!
//! **P31.3 — physics-shape-aware colliders.** A physical prim starts with a
//! placeholder cuboid collider sized to its prim scale. Once the object's
//! `LLPhysicsShapeType` is known — fetched via the `GetObjectPhysicsData`
//! capability ([`Command::RequestObjectPhysicsData`], requested by
//! `request_object_physics_data`) and folded, with any unsolicited
//! `ObjectPhysicsProperties` pushes, into `ObjectPhysicsShapes` by
//! `ingest_object_physics_data` — `refine_physical_colliders` builds the
//! matching [`parry3d`] [`SharedShape`] from the geometry the viewer already
//! tessellates (the object's own `GeometryHolder` faces, so linkset children
//! are excluded): **none** → no collider; **convex hull** → a convex hull of the
//! prim / mesh vertices; **prim** → a trimesh of that geometry. These shapes are
//! published each frame into the moving-collider set
//! ([`crate::raycast_index::DynamicColliders`]) so camera collision and the
//! prim–prim collision sounds see them.
//!
//! **P31.4 — avatar dead-reckoning.** The same `interpolateLinearMotion` port is
//! extended to the own and other full-object avatars (the [`sl_viewer_world_objects::avatars`] path,
//! not the object path): `apply_object`(sl_viewer_world_objects::avatars) stamps each avatar's
//! anchor with an [`AvatarMotion`] marker, and `drive_avatar_motion` dead-reckons
//! it between updates with the same phase-out taper and geometric clamps — but with
//! the **stricter avatar ground floor** (`avatar_ground_floor`:
//! `land + 0.5 * height`) so a laggy avatar does not sink under the terrain. The
//! shared `MotionState` + `advance_motion` step drive both the object and avatar
//! paths; they differ only in that ground floor. Avatars stay kinematic
//! (sim-authoritative); the predicted motion is applied to the anchor as a
//! translation delta so the root-drop render offset (R23) is preserved.
//!
//! No `sl-client-tokio` counterpart is needed: like the render materials and the
//! other viewer-only simulations (sky, water, particles), the physics world is a
//! viewer rendering concern, not a protocol capability, so the runtime-parity
//! rule does not apply.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use parry3d::math::{Pose as ParryPose, Vector as ParryVec};
use parry3d::shape::SharedShape;
use sl_client_bevy::{
    Command, MeshKey, MeshPhysics, ObjectKey, ObjectPhysicsData, PhysicsShapeType, RegionHandle,
    SlCommand, SlEvent, SlIdentity, SlSessionEvent, Submesh, Vector,
};

use crate::avatars::update_avatar_objects;
use crate::coords::{region_offset_bevy, sl_rotation_to_quat, sl_to_bevy_rotation, sl_to_bevy_vec};
use crate::meshes::MeshManager;
use crate::objects::{GeometryHolder, ObjectCategory, ObjectSlMotion, SceneObject, update_objects};
use crate::raycast_index::{DynamicColliders, RaycastIndexColliders};
use crate::world_api::ObjectState;

use crate::world_api::AvatarState;
use crate::world_api::TerrainState;
use crate::world_api::{
    AvatarInterp, AvatarMotion, MotionState, PhysicalObject, ViewerCamera, bevy_rotation_of,
};

/// Clamp a raw region time dilation into the `0.0..=1.0` speed factor the
/// dead-reckoning step multiplies by. Guard a non-finite value (falling back to
/// a healthy `1.0`) and clamp into range so a malformed update can never poison
/// the prediction.
#[must_use]
const fn clamp_dilation(dilation: f32) -> f32 {
    if dilation.is_finite() {
        dilation.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// The most recent `RegionData.TimeDilation` seen per region, folded from the
/// session event stream by `ingest_time_dilation` and read (for the agent's
/// current region) by `drive_physical_objects`.
#[derive(Resource, Default)]
pub(crate) struct RegionTimeDilation {
    /// The latest dilation (`0.0..=1.0`) for each region, keyed by handle.
    per_region: HashMap<RegionHandle, f32>,
}

/// The viewer's physics plugin: dead-reckoning of kinematic movers + building
/// collision geometry for the custom raycast index. No physics *engine* — see
/// the module docs; the viewer simulates nothing, so there is no solver, no
/// gravity resource, and no fixed-step schedule to configure.
#[derive(Debug)]
pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RegionTimeDilation>()
            .init_resource::<CircuitLiveness>()
            .init_resource::<ObjectPhysicsShapes>()
            .init_resource::<StaticColliderBuilds>()
            .add_systems(
                Update,
                (
                    ingest_time_dilation,
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
                    // Seated avatars ride their seat, not the region: place them from
                    // the seat's current-frame world transform (composed from local
                    // transforms — the seat's `GlobalTransform` is a frame stale), so
                    // this must run after every mover that writes a seat's local
                    // transform this frame — `update_objects` (authoritative snap) and
                    // `drive_physical_objects` (between-update dead-reckon) — and after
                    // `drive_avatar_motion` (whose write it overrides for a seated
                    // anchor), and before the camera follow reads the own avatar's pose.
                    crate::avatars::place_seated_avatars
                        .after(update_objects)
                        .after(drive_physical_objects)
                        .after(drive_avatar_motion)
                        .before(crate::camera::position_camera),
                    // P31.3: replace the placeholder cuboid with a shape-aware
                    // collider once the physics data and geometry are available.
                    refine_physical_colliders
                        .after(update_objects)
                        .after(drive_physical_objects)
                        .after(ingest_object_physics_data),
                    // Strip a stale static collider from a prim that just became a
                    // physical root (the physical path now owns its collider). Runs
                    // before the physical drivers install the kinematic collider, so a
                    // static→physical transition in one frame does not have this remove
                    // the collider the physical path just added.
                    detach_static_colliders
                        .after(update_objects)
                        .before(drive_physical_objects)
                        .before(refine_physical_colliders),
                    // Insert finished off-thread collider builds before the scanner
                    // re-scans (so a completed build clears its in-flight slot first).
                    apply_static_colliders.after(update_objects),
                    // Populate the scene collision geometry: give every non-physical,
                    // non-avatar prim a static collider (budgeted, nearest-first), so
                    // the raycast index finds walls / floors for camera collision and
                    // the "objects near X" broad phase (viewer-physics-static-prim-colliders).
                    // Collider construction runs off-thread (`apply_static_colliders`
                    // installs the result).
                    build_static_colliders
                        .after(update_objects)
                        .after(detach_static_colliders)
                        .after(apply_static_colliders)
                        .after(ingest_object_physics_data),
                    // Mirror each installed static collider into the custom
                    // off-thread raycast index (viewer-perf-custom-static-raycast-index),
                    // the replacement for avian's `SpatialQuery`. Change-driven — only
                    // added / moved / removed colliders touch it — so it never re-scans
                    // the whole prim set. Runs after the collider is installed.
                    sync_raycast_index
                        .after(apply_static_colliders)
                        .after(detach_static_colliders),
                    // Refill the moving-collider set from the physical prims' current
                    // poses each frame (camera collision + collision sounds see them),
                    // after their collider shapes and world poses are up to date.
                    sync_dynamic_colliders
                        .after(refine_physical_colliders)
                        .after(drive_physical_objects),
                    // Diagnostic (SL_VIEWER_LOG_CAMERA_COLLISION=1): list colliders
                    // near the avatar to hunt a stray one the camera pulls in on.
                    log_colliders_near_avatar,
                ),
            );
    }
}

/// Fold each region's most recent `TimeDilation` into `RegionTimeDilation`.
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

/// Exponential-smoothing time constant (seconds) for easing a physical object's
/// *rendered* pose toward its authoritative / dead-reckoned pose — the object
/// counterpart of [`ROTATION_SMOOTHING_TAU_SECS`]. Unlike the avatar rotation
/// ease (which slerps the absolute facing toward a mostly-static target), the
/// object path eases the per-update *correction* as a decaying residual
/// ([`PhysicsInterp::render_offset`] / [`PhysicsInterp::render_rot_offset`]), so a
/// steadily dead-reckoned vehicle carries no velocity-proportional standing lag —
/// only the divergence between prediction and the next authoritative update is
/// smoothed away. A ~100 ms constant hides the per-update snap (the object
/// rubberband) while staying responsive, and turns a keyframed
/// (velocity-less) vehicle's discrete per-update jumps into a continuous glide.
const OBJECT_SMOOTHING_TAU_SECS: f32 = 0.1;

/// If an authoritative update leaves a rendered-vs-truth residual larger than this
/// (metres), the object is **snapped** rather than eased: a region crossing /
/// rebase or a scripted teleport, where easing would visibly slide the object
/// across the gap. A normal per-update prediction correction is far smaller than a
/// region-scale jump, so this cleanly separates the two.
const OBJECT_SNAP_DISTANCE_M: f32 = 32.0;

/// Advance a `MotionState` one dead-reckoning frame, exactly as the reference
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
/// `drive_physical_objects`: the extrapolated (predicted) pose advanced each
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
    /// The residual between the last *rendered* position and the authoritative /
    /// dead-reckoned position (Bevy space), decayed toward zero each frame so a
    /// per-update correction eases in instead of snapping (the translation
    /// counterpart of the avatar's P31.7 rotation ease). Zero in steady
    /// dead-reckoned motion (prediction ≈ truth); non-zero only just after a
    /// divergent update, then absorbed over [`OBJECT_SMOOTHING_TAU_SECS`].
    render_offset: Vec3,
    /// The residual between the last *rendered* orientation and the authoritative /
    /// dead-reckoned orientation (a Bevy-space world-rotation correction applied as
    /// `render_rot_offset * predicted`), decayed toward identity each frame — the
    /// rotation counterpart of [`PhysicsInterp::render_offset`], so a turning
    /// vehicle's per-update rotation snap eases away too.
    render_rot_offset: Quat,
    /// The rest latch (the flexi settle-latch pattern): `Some(region_offset)` once
    /// the authoritative motion is stationary and the render residuals have
    /// settled — the pose was written exactly once and the per-frame drive is
    /// skipped, so a parked physical prim stops dirtying its `Transform` every
    /// frame. The latched scene-origin offset is stored because the per-frame
    /// write *is* the re-baser across an origin move: a latched prim wakes when
    /// the offset changes (and on any fresh server update via [`Self::reseed`]).
    rest: Option<Vec3>,
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
            // A freshly-seeded object renders exactly at truth (no correction yet).
            render_offset: Vec3::ZERO,
            render_rot_offset: Quat::IDENTITY,
            rest: None,
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
        // later collider resize by `refine_physical_colliders`) track a resize.
        self.collider_scale = collider_extents(&phys.scale);
        // Fresh server truth wakes a latched prim: the update may set it moving.
        self.rest = None;
    }
}

/// The per-axis motion magnitude (m/s, m/s², rad/s) below which a dead-reckoned
/// physical prim counts as stationary for the rest latch. A settled server object
/// reports exact zeros (and the phase-out taper multiplies the prediction to
/// exactly zero at its end), so this margin only has to absorb denormal noise.
const REST_MOTION_EPSILON: f32 = 1.0e-4;

/// The squared render-residual length (metres²) below which the positional
/// correction counts as absorbed for the rest latch (~0.1 mm — far below a
/// pixel at any usable camera distance).
const REST_OFFSET_EPSILON_SQ: f32 = 1.0e-8;

/// The squared length of the residual quaternion's vector part below which the
/// rotation correction counts as absorbed for the rest latch: `|xyz| ≈ angle/2`
/// for a small rotation, so `5e-5` is ~1e-4 rad (~0.006°) — invisible. (A cosine
/// margin on `w` cannot express this: `1 - 1e-8` is not representable in `f32`.)
const REST_ROTATION_EPSILON_SQ: f32 = 2.5e-9;

/// Whether a physical prim's dead-reckoned motion and render residuals have
/// settled enough to latch it at rest ([`PhysicsInterp::rest`]): the
/// authoritative motion is stationary on every axis and both residual
/// corrections are visually fully absorbed. Pure, so it is unit-testable.
fn physical_object_settled(
    motion: &MotionState,
    render_offset: Vec3,
    render_rot_offset: Quat,
) -> bool {
    let stationary = motion
        .velocity
        .iter()
        .chain(motion.acceleration.iter())
        .chain(motion.angular_velocity.iter())
        .all(|component| component.abs() < REST_MOTION_EPSILON);
    let residual_axis = Vec3::new(
        render_rot_offset.x,
        render_rot_offset.y,
        render_rot_offset.z,
    );
    stationary
        && render_offset.length_squared() < REST_OFFSET_EPSILON_SQ
        && residual_axis.length_squared() < REST_ROTATION_EPSILON_SQ
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

/// Exponential-smoothing time constant (seconds) for easing the avatar's *rendered*
/// orientation toward its authoritative / dead-reckoned facing (P31.7). The own
/// avatar's facing arrives only as sparse `ObjectUpdate`s echoing the client-driven
/// `SetRotation` (P31.5, throttled to ~20 Hz and coarser once the sim re-broadcasts
/// it), so the target jumps in steps; a ~80 ms constant smooths those steps into a
/// fluid turn while staying responsive (it covers ~63 % of a step in one constant,
/// ~95 % in three) and converges to the target once turning stops, leaving no
/// standing lag.
const ROTATION_SMOOTHING_TAU_SECS: f32 = 0.08;

/// The time constant (seconds) for easing the avatar anchor's rendered
/// **translation** toward the authoritative / dead-reckoned position, the
/// translation counterpart of [`ROTATION_SMOOTHING_TAU_SECS`]. Kept short (~100 ms)
/// so the own avatar's rendered position stays responsive to input while the
/// per-update correction that used to hard-snap (the dead-reckoning rubberband) is
/// spread across a few frames instead of jumping in one.
const TRANSLATION_SMOOTHING_TAU_SECS: f32 = 0.1;

/// The distance (metres) beyond which a fresh authoritative avatar position is
/// **snapped** rather than eased: a region crossing's 256 m rebase or a teleport
/// must not glide across, exactly as the object path snaps past
/// [`OBJECT_SNAP_DISTANCE_M`]. Region-scale, so ordinary per-update prediction
/// error (sub-metre to a few metres) always eases.
const TRANSLATION_SNAP_DISTANCE_M: f32 = 32.0;

/// The framerate-independent exponential-smoothing blend factor for a frame of
/// length `dt` seconds and time constant `tau_secs` (`1 - e^(-dt/τ)`). A
/// non-positive `dt` blends fully (snap) so a paused / first frame cannot stall.
fn smoothing_alpha(dt: f32, tau_secs: f32) -> f32 {
    if dt <= 0.0 {
        return 1.0;
    }
    1.0 - (-dt / tau_secs).exp()
}

/// The exponential-smoothing blend factor for a frame of length `dt` seconds
/// (`1 - e^(-dt/τ)`), the framerate-independent easing toward the target facing. A
/// non-positive `dt` blends fully (snap) so a paused / first frame cannot stall.
fn rotation_smoothing_alpha(dt: f32) -> f32 {
    smoothing_alpha(dt, ROTATION_SMOOTHING_TAU_SECS)
}

/// The exponential-smoothing blend factor for a frame of length `dt` seconds, the
/// framerate-independent easing of the rendered translation toward the
/// authoritative / dead-reckoned position.
fn translation_smoothing_alpha(dt: f32) -> f32 {
    smoothing_alpha(dt, TRANSLATION_SMOOTHING_TAU_SECS)
}

/// The squared residual distance (metres²) below which [`eased_translation`]
/// converges exactly (~0.1 mm): the exponential ease is asymptotic and `f32`
/// rounding can stall it just short of the target, which would keep the anchor
/// `Transform` marked changed — and its whole subtree re-propagated — forever.
const TRANSLATION_SETTLE_EPSILON_SQ: f32 = 1.0e-8;

/// The next eased rendered anchor translation, given the last `rendered`
/// position, the current authoritative/dead-reckoned `target`, whether a region
/// crossing occurred this frame, and the frame's easing `alpha`. Called every
/// frame (the ease is continuous, not update-gated).
///
/// Snaps (returns `target`) across a region crossing or any region-scale jump
/// (a teleport / 256 m rebase) so the world does not glide across it; otherwise
/// eases from `rendered` toward `target`, spreading an ordinary per-update
/// prediction correction over a few frames instead of the visible hard snap —
/// with a terminal snap once the residual is sub-visible, so an idle avatar's
/// anchor actually reaches equality and settles.
fn eased_translation(rendered: Vec3, target: Vec3, region_crossed: bool, alpha: f32) -> Vec3 {
    if region_crossed || target.distance(rendered) > TRANSLATION_SNAP_DISTANCE_M {
        target
    } else {
        let next = rendered.lerp(target, alpha);
        if next.distance_squared(target) < TRANSLATION_SETTLE_EPSILON_SQ {
            target
        } else {
            next
        }
    }
}

/// The per-component margin for the rotation ease's terminal snap: an eased
/// quaternion within this of the target on every component is a sub-0.005°
/// residual — invisible, so the slerp converges exactly instead of stalling a
/// hair short (which would keep the anchor marked changed forever). (A cosine
/// margin cannot express this in `f32`: `1 - 1e-8` rounds to `1.0`.)
const ROTATION_SETTLE_EPSILON: f32 = 1.0e-5;

/// Ease the avatar anchor's rendered orientation toward its current authoritative /
/// dead-reckoned facing and return the value to write (P31.7), or `None` for a
/// placeholder sphere (which does not carry the object rotation), so only rigged
/// bodies smooth-turn. `dt` is the real (undilated) frame time — the smoothing is a
/// visual concern, independent of the physics clock.
///
/// Deliberately returns the rotation instead of taking the anchor's `Transform`:
/// passing a `Mut<Transform>` into a `&mut Transform` parameter deref-mut-coerces
/// and marks the component changed **every call**, no matter what is written
/// inside — which kept every idle avatar's anchor dirty per frame and defeated
/// the pose gate. The caller writes through the `Mut` only on an actual change.
fn smoothed_rotation(interp: &mut AvatarInterp, dt: f32) -> Option<Quat> {
    if !interp.apply_rotation {
        return None;
    }
    let target = bevy_rotation_of(&interp.motion);
    let alpha = rotation_smoothing_alpha(dt);
    let next = interp.rendered_rotation.slerp(target, alpha);
    interp.rendered_rotation = if next.abs_diff_eq(target, ROTATION_SETTLE_EPSILON) {
        target
    } else {
        next
    };
    Some(interp.rendered_rotation)
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
    // The scene origin, so a physical prim in a neighbour region is placed onto the
    // right terrain and re-based across a crossing (like avatars and static objects).
    let origin = terrain.origin();
    // The circuit looks stalled if no inbound event has been seen for longer than
    // the phase-out window (the analogue of `isBlocked` / a stale last-packet time).
    let circuit_stale = liveness
        .last_event_secs
        .is_some_and(|seen| now - seen > PHASE_OUT_START_SECS);

    for (entity, phys, interp, mut transform) in &mut objects {
        let Some(mut interp) = interp else {
            // Newly physical: seed the interpolation and place the entity at the
            // authoritative pose. The collision shape is owned by
            // `refine_physical_colliders` (a placeholder cuboid until the physics
            // shape and geometry arrive) and published to the moving-collider set
            // by `sync_dynamic_colliders`.
            debug!("physical object {entity} → kinematic mover");
            place(
                &mut transform,
                &phys.position,
                &sl_rotation_to_quat(&phys.rotation),
                region_offset_bevy(phys.region_handle, origin),
            );
            commands
                .entity(entity)
                .insert(PhysicsInterp::seeded(&phys, now));
            continue;
        };

        // A fresh server update: reseed the prediction to truth and restart the
        // timers. Rather than *snapping* the rendered pose to truth (the visible
        // per-update rubberband), re-aim the decaying residual so the object eases
        // toward truth from wherever it was rendered last frame — the translation +
        // rotation counterpart of the avatar's P31.7 rotation ease. The collider
        // itself (including a rebuild on resize) is owned by
        // `refine_physical_colliders`; `reseed` only refreshes the cached scale.
        if phys.is_changed() {
            let predicted_pos = bevy_position_of(&interp.motion);
            let rendered_pos_before = Vec3::new(
                predicted_pos.x + interp.render_offset.x,
                predicted_pos.y + interp.render_offset.y,
                predicted_pos.z + interp.render_offset.z,
            );
            let rendered_rot_before = interp
                .render_rot_offset
                .mul_quat(bevy_rotation_of(&interp.motion));
            interp.reseed(&phys, now);
            reaim_residual(&mut interp, rendered_pos_before, rendered_rot_before);
            let region_offset = region_offset_bevy(interp.motion.region_handle, origin);
            place_smoothed(&mut interp, &mut transform, dt_raw, region_offset);
            continue;
        }

        // At rest (the flexi settle-latch pattern): a stationary prim whose
        // residuals are absorbed was written exactly once at latch time, so
        // skip the dead-reckon and the per-frame Transform write entirely. The
        // latched scene-origin offset must still match — the per-frame write is
        // the re-baser across an origin move — and any fresh server update
        // cleared the latch above (`reseed`).
        let region = interp.motion.region_handle;
        let region_offset = region_offset_bevy(region, origin);
        if let Some(rest_offset) = interp.rest {
            if rest_offset == region_offset {
                continue;
            }
            interp.rest = None;
        }

        // Between updates: dead-reckon forward.
        let region_dilation = dilations.per_region.get(&region).copied().unwrap_or(1.0);
        let dt = clamp_dilation(region_dilation) * dt_raw;
        let time_since_last_update = now - interp.last_message_secs;
        if dt <= 0.0 || time_since_last_update <= 0.0 {
            interp.last_interp_secs = now;
            // Keep easing any outstanding residual even on a stalled physics frame.
            place_smoothed(&mut interp, &mut transform, dt_raw, region_offset);
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
        // Render the freshly dead-reckoned pose, easing away any residual left over
        // from the last authoritative correction (zero in steady prediction).
        place_smoothed(&mut interp, &mut transform, dt_raw, region_offset);

        // Settled? Zero the residuals exactly, write the exact pose one final
        // time, and latch — the next frames skip this prim entirely until a
        // server update or an origin move wakes it.
        if physical_object_settled(
            &interp.motion,
            interp.render_offset,
            interp.render_rot_offset,
        ) {
            interp.render_offset = Vec3::ZERO;
            interp.render_rot_offset = Quat::IDENTITY;
            place_smoothed(&mut interp, &mut transform, dt_raw, region_offset);
            interp.rest = Some(region_offset);
        }
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
fn place(transform: &mut Transform, position: &Vector, sl_rotation: &Quat, region_offset: Vec3) {
    let local = sl_to_bevy_vec(position);
    // `region_offset` places a physical prim in a neighbour region onto the right
    // terrain (zero for the root region); added only to the final translation, so
    // the residual math stays in region-local space and self-corrects the offset
    // every frame when the scene origin moves (a crossing).
    transform.translation = Vec3::new(
        local.x + region_offset.x,
        local.y + region_offset.y,
        local.z + region_offset.z,
    );
    transform.rotation = sl_to_bevy_rotation().mul_quat(*sl_rotation);
}

/// The Bevy-world position of a predicted motion: its Second Life region-local
/// position carried through the single Second Life → Bevy basis change (the same
/// mapping [`place`] applies).
fn bevy_position_of(motion: &MotionState) -> Vec3 {
    sl_to_bevy_vec(&array_to_vector(motion.position))
}

/// Decay a physical object's render residual one frame and write the resulting
/// **rendered** pose (predicted pose composed with the decaying residual) into its
/// [`Transform`]. `dt` is the real (undilated) frame time — the smoothing is a
/// visual concern, independent of the physics clock. In steady dead-reckoned
/// motion the residual is zero, so the rendered pose is the prediction; just after
/// a divergent authoritative update it eases the correction in over
/// [`OBJECT_SMOOTHING_TAU_SECS`] instead of snapping (the object rubberband fix).
fn place_smoothed(
    interp: &mut PhysicsInterp,
    transform: &mut Transform,
    dt: f32,
    region_offset: Vec3,
) {
    let alpha = smoothing_alpha(dt, OBJECT_SMOOTHING_TAU_SECS);
    // Decay the residual toward zero (position) / identity (rotation).
    interp.render_offset = interp.render_offset.lerp(Vec3::ZERO, alpha);
    interp.render_rot_offset = interp.render_rot_offset.slerp(Quat::IDENTITY, alpha);
    let predicted_pos = bevy_position_of(&interp.motion);
    let predicted_rot = bevy_rotation_of(&interp.motion);
    // `region_offset` is applied only to the final translation (a neighbour
    // region's prim onto the right terrain, zero for the root region); the
    // residual (`render_offset`) stays in region-local space, so a crossing that
    // moves the origin self-corrects here each frame without re-basing the interp.
    transform.translation = Vec3::new(
        predicted_pos.x + interp.render_offset.x + region_offset.x,
        predicted_pos.y + interp.render_offset.y + region_offset.y,
        predicted_pos.z + interp.render_offset.z + region_offset.z,
    );
    transform.rotation = interp.render_rot_offset.mul_quat(predicted_rot).normalize();
}

/// Re-aim a physical object's render residual so its rendered pose stays continuous
/// across an authoritative re-seed: the residual is set to the gap between the pose
/// rendered *last* frame (`rendered_*_before`) and the fresh truth
/// (`interp.motion`, already re-seeded), so the ease continues from where the object
/// visibly was rather than jumping to truth. A gap larger than
/// [`OBJECT_SNAP_DISTANCE_M`] is a discontinuity (region crossing / rebase /
/// teleport): the residual is zeroed so the object snaps to truth instead of sliding.
fn reaim_residual(
    interp: &mut PhysicsInterp,
    rendered_pos_before: Vec3,
    rendered_rot_before: Quat,
) {
    let predicted_pos = bevy_position_of(&interp.motion);
    let predicted_rot = bevy_rotation_of(&interp.motion);
    let offset = Vec3::new(
        rendered_pos_before.x - predicted_pos.x,
        rendered_pos_before.y - predicted_pos.y,
        rendered_pos_before.z - predicted_pos.z,
    );
    if offset.length() > OBJECT_SNAP_DISTANCE_M {
        // A region-scale jump: snap (render at truth), don't slide across the gap.
        interp.render_offset = Vec3::ZERO;
        interp.render_rot_offset = Quat::IDENTITY;
    } else {
        interp.render_offset = offset;
        interp.render_rot_offset = rendered_rot_before.mul_quat(predicted_rot.inverse());
    }
}

/// Strip the dead-reckoning state from an entity that is no longer a physical
/// root (its [`PhysicalObject`] marker was removed by `apply_physics`
/// ([`sl_viewer_world_objects::objects`]) — e.g. a
/// prim made non-physical, relinked as a child, or attached), so it stops being
/// driven. Dropping [`RefinedCollider`] also drops it from the moving-collider
/// set on the next [`sync_dynamic_colliders`] pass.
pub(crate) fn detach_physical_bodies(
    stale: Query<Entity, (With<PhysicsInterp>, Without<PhysicalObject>)>,
    mut commands: Commands,
) {
    for entity in &stale {
        commands
            .entity(entity)
            .remove::<(PhysicsInterp, RefinedCollider)>();
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

/// A collision plane whose up-axis component is below this is treated as absent (a
/// near-vertical plane cannot be a floor, and dividing by its tiny `nz` would
/// explode). Mirrors [`sl_viewer_world_objects::ground`]'s guard.
const PLANE_NORMAL_EPSILON: f32 = 1.0e-3;

/// `SL_VIEWER_LOG_AVATAR_GROUND=1` traces the avatar ground floor
/// ([`avatar_collision_floor`]): the reported / floored Second Life Z, the land
/// height and whether a collision plane was present, the resolved floor, and the
/// anchor's target vs rendered Y. Fires only when the floor actually lifted the
/// avatar (or a plane was present), so a settled avatar does not flood the trace.
/// Silent by default; read once.
fn log_avatar_ground_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("SL_VIEWER_LOG_AVATAR_GROUND")
            .is_ok_and(|value| value == "1" || value == "true")
    })
}

/// The Second Life **capsule-centre Z** floor for an avatar at region-local
/// `(x, y)`: the terrain land height and, when the simulator reports a collision
/// (foot) plane, that plane's height there — whichever is higher — each lifted by
/// half the avatar's height (the reported position is the capsule centre, the feet
/// `0.5·height` below it). `None` when neither is known.
///
/// This is the authoritative-ground version of `avatar_ground_floor`: the
/// simulator's foot plane is the surface it says the avatar is standing on
/// (including a prim floor above the terrain, and present even while the region's
/// land patches are mid-rebuild), so flooring the rendered avatar to it is what
/// keeps a bouncing / low authoritative position from dropping the avatar through
/// the ground — the same plane the reference viewer plants feet on
/// ([`sl_viewer_world_objects::ground`]).
fn avatar_collision_floor(
    plane: Option<[f32; 4]>,
    land_height: Option<f32>,
    x: f32,
    y: f32,
    height: f32,
) -> Option<f32> {
    let plane_z = plane.and_then(|[nx, ny, nz, w]| {
        (nz.abs() >= PLANE_NORMAL_EPSILON).then(|| (w - nx * x - ny * y) / nz)
    });
    let ground = match (land_height, plane_z) {
        (Some(land), Some(plane_z)) => Some(land.max(plane_z)),
        (Some(land), None) => Some(land),
        (None, Some(plane_z)) => Some(plane_z),
        (None, None) => None,
    };
    ground.map(|ground| ground + 0.5 * height)
}

/// Dead-reckon every full-object avatar between server updates (P31.4), the avatar
/// counterpart of `drive_physical_objects`: on the frame an [`AvatarMotion`]
/// update lands, `apply_object`(sl_viewer_world_objects::avatars) has already snapped the anchor to
/// the authoritative pose, so this only (re)seeds the interpolation; between
/// updates it advances the predicted pose with the same phase-out taper and
/// geometric clamps as the object path — but with the **stricter avatar ground
/// floor** (`avatar_ground_floor`) so a laggy avatar does not sink under the
/// terrain. The avatar stays kinematic (sim-authoritative); the predicted motion is
/// applied to the anchor as a translation *delta* (plus, for a rigged body, the
/// predicted orientation), leaving the root-drop render offset intact.
#[expect(
    clippy::type_complexity,
    reason = "the dead-reckoner's avatar query — motion, interpolation state and anchor transform \
              — with a `Without<Seated>` filter so a seat-driven avatar is left alone"
)]
pub(crate) fn drive_avatar_motion(
    time: Res<Time>,
    liveness: Res<CircuitLiveness>,
    dilations: Res<RegionTimeDilation>,
    terrain: Res<TerrainState>,
    mut avatars: Query<
        (
            Entity,
            Ref<AvatarMotion>,
            Option<&mut AvatarInterp>,
            &mut Transform,
        ),
        // A seated avatar rides its seat, not the region: `place_seated_avatars`
        // drives its anchor, so the region-space dead-reckoner must leave it be.
        Without<crate::world_api::Seated>,
    >,
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
            commands.entity(entity).insert(AvatarInterp::seeded(
                &motion,
                now,
                transform.translation,
            ));
            continue;
        };

        // Update the authoritative/predicted `target_translation` this frame, and
        // note whether it moved discontinuously (a region crossing) so the ease
        // below snaps rather than glides.
        let mut region_crossed = false;
        if motion.is_changed() {
            // A fresh server update: `apply_object` already snapped the anchor to
            // the authoritative pose, so capture that as the new target.
            region_crossed = interp.motion.region_handle != motion.region_handle;
            interp.target_translation = transform.translation;
            interp.reseed(&motion, now);
        } else {
            // Between updates: dead-reckon forward and advance the target by the
            // prediction delta (a Bevy-space delta, so the root-drop render offset
            // R23 baked into the target is preserved).
            let region = interp.motion.region_handle;
            let region_dilation = dilations.per_region.get(&region).copied().unwrap_or(1.0);
            let dt = clamp_dilation(region_dilation) * dt_raw;
            let time_since_last_update = now - interp.last_message_secs;
            if dt > 0.0 && time_since_last_update > 0.0 {
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
                    // Avatars use the stricter ground floor so a laggy avatar does
                    // not sink under the terrain (the one guard the object path
                    // leaves permissive).
                    |predicted_x, predicted_y| {
                        avatar_ground_floor(
                            terrain.land_height(region, predicted_x, predicted_y),
                            height,
                        )
                    },
                );
                let [prev_x, prev_y, prev_z] = previous;
                let [next_x, next_y, next_z] = interp.motion.position;
                let delta = sl_to_bevy_vec(&Vector {
                    x: next_x - prev_x,
                    y: next_y - prev_y,
                    z: next_z - prev_z,
                });
                interp.target_translation = Vec3::new(
                    interp.target_translation.x + delta.x,
                    interp.target_translation.y + delta.y,
                    interp.target_translation.z + delta.z,
                );
            }
            interp.last_interp_secs = now;
        }

        // Floor the avatar to the simulator's **authoritative ground** — the terrain
        // land height and, when present, the collision (foot) plane the sim reports
        // (the surface it says the avatar's feet are on, including a prim floor above
        // the terrain). Applied to the authoritative *and* dead-reckoned position, so
        // a bouncing / low reported Z (an unstable avatar the sim reports metres below
        // the ground it is standing on) can never drop the avatar through it. The
        // anchor's Bevy Y is a constant offset from the Second Life capsule Z (root
        // drop + region offset), so raising the capsule Z by `Δ` raises the anchor Y
        // by the same `Δ` — no root-drop maths needed. Released (`None`) only when the
        // sim reports no ground at all (airborne over an un-ingested region).
        let [px, py, reported_z] = interp.motion.position;
        let land = terrain.land_height(interp.motion.region_handle, px, py);
        let plane_present = motion.collision_plane().is_some();
        let floor_z = avatar_collision_floor(motion.collision_plane(), land, px, py, interp.height);
        let floor_bevy_y = floor_z.map(|floor_z| {
            // The Bevy anchor Y of the floor, via the constant `anchor.y − capsule.z`.
            let floor_y = interp.target_translation.y + (floor_z - reported_z);
            interp.motion.position = [px, py, reported_z.max(floor_z)];
            interp.target_translation.y = interp.target_translation.y.max(floor_y);
            floor_y
        });

        // Ease the rendered translation toward the target *every* frame (the
        // translation counterpart of the orientation easing, P31.7) so a per-update
        // correction spreads over a few frames instead of hard-snapping (the
        // dead-reckoning rubberband) — and, crucially, so a short teleport that
        // leaves the avatar standing still still converges fully to the destination
        // rather than freezing part-way once updates stop. A region crossing /
        // teleport-scale jump snaps instead of gliding.
        interp.rendered_translation = eased_translation(
            interp.rendered_translation,
            interp.target_translation,
            region_crossed,
            translation_smoothing_alpha(dt_raw),
        );
        // Hard-floor the rendered height (no eased glide up from a sink): the avatar
        // must never render below the authoritative ground even mid-ease.
        if let Some(floor_y) = floor_bevy_y {
            interp.rendered_translation.y = interp.rendered_translation.y.max(floor_y);
        }

        if log_avatar_ground_enabled() {
            // Only when the floor lifted the avatar (the fall-through it prevents) or a
            // plane was present (so a trapped jump/fall would show up as a lift while
            // airborne), to keep the trace readable.
            let lifted = floor_z.is_some_and(|floor_z| reported_z < floor_z - 0.01);
            if lifted || plane_present {
                let floored_z = floor_z.map_or(reported_z, |floor_z| reported_z.max(floor_z));
                info!(
                    "avatar-ground {entity}: reported_z={reported_z:.3} floored_z={floored_z:.3} \
                     land={land:?} plane={plane_present} floor_z={floor_z:?} lifted={lifted} \
                     target_y={:.3} rendered_y={:.3} update={}",
                    interp.target_translation.y,
                    interp.rendered_translation.y,
                    motion.is_changed(),
                );
            }
        }
        // Write-on-change: once the ease's terminal snap converges, an idle
        // avatar's anchor stops being marked changed — which stops Bevy's
        // change-gated propagation re-walking its ~200-entity subtree every
        // frame (and is the precondition for skipping the skeleton driver).
        if transform.translation != interp.rendered_translation {
            transform.translation = interp.rendered_translation;
        }
        if let Some(rotation) = smoothed_rotation(&mut interp, dt_raw)
            && transform.rotation != rotation
        {
            transform.rotation = rotation;
        }
    }
}

/// The per-object physics-shape data the viewer has learned, keyed by full
/// [`ObjectKey`] (the id the `GetObjectPhysicsData` capability reply uses). Folded
/// by `ingest_object_physics_data` from both the capability reply
/// ([`SlSessionEvent::ObjectPhysicsData`]) and the unsolicited event-queue push
/// ([`SlSessionEvent::ObjectPhysicsProperties`]), and read by
/// `refine_physical_colliders` to pick each physical object's collision shape.
#[derive(Resource, Default)]
pub(crate) struct ObjectPhysicsShapes {
    /// The latest physics data for each object, keyed by full key.
    data: HashMap<ObjectKey, ObjectPhysicsData>,
    /// The objects a `GetObjectPhysicsData` request has already been sent for, so
    /// `request_object_physics_data` asks the grid exactly once per object.
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
/// `ObjectPhysicsShapes`: the `GetObjectPhysicsData` capability reply (already
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

/// Records the current collision [`SharedShape`] (and what shape / scale it was
/// built for) of a physical object, so `refine_physical_colliders` rebuilds it
/// only when the shape data, the geometry, or the scale actually change, and
/// [`sync_dynamic_colliders`] publishes it into the moving-collider set.
#[derive(Component)]
pub(crate) struct RefinedCollider {
    /// The parry collision shape at the current scale, or [`None`] for a
    /// `PhysicsShapeType::None` prim (no collider).
    collider: Option<SharedShape>,
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
            for &[a, b, c] in values.as_chunks::<3>().0 {
                out.push([
                    base.saturating_add(u32::from(a)),
                    base.saturating_add(u32::from(b)),
                    base.saturating_add(u32::from(c)),
                ]);
            }
        }
        Indices::U32(values) => {
            for &[a, b, c] in values.as_chunks::<3>().0 {
                out.push([
                    base.saturating_add(a),
                    base.saturating_add(b),
                    base.saturating_add(c),
                ]);
            }
        }
    }
}

/// Gather a physical object's own tessellated geometry — the faces under its
/// `GeometryHolder` child, **excluding** the linkset child prims that also parent
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
/// physics-shape data (`ObjectPhysicsShapes`) and the object's tessellated
/// geometry are available:
///
/// - **unknown** (data not yet in) → keep the placeholder cuboid;
/// - **none** ([`PhysicsShapeType::None`]) → no collider (a physical prim that
///   collides with nothing);
/// - **convex hull** → a convex hull of the prim / mesh vertices;
/// - **prim** (or an unrecognised type) → a trimesh of that geometry.
///
/// The [`SharedShape`] is recorded in [`RefinedCollider`] (rebuilt only on a real
/// change — new shape data, a resize, or geometry finally arriving) and published
/// each frame into the moving-collider set by [`sync_dynamic_colliders`], so
/// camera collision and the prim–prim collision sounds see the physical prims.
#[expect(
    clippy::too_many_arguments,
    reason = "an ECS system's arguments are its injected queries / resources"
)]
pub(crate) fn refine_physical_colliders(
    shapes: Res<ObjectPhysicsShapes>,
    object_state: Res<ObjectState>,
    mut mesh_manager: ResMut<MeshManager>,
    objects: Query<
        (
            Entity,
            &SceneObject,
            &PhysicalObject,
            Option<&RefinedCollider>,
        ),
        With<PhysicsInterp>,
    >,
    children_q: Query<&Children>,
    holders: Query<(), With<GeometryHolder>>,
    mesh_handles: Query<&Mesh3d>,
    meshes: Res<Assets<Mesh>>,
    mut commands: Commands,
) {
    for (entity, scene, phys, existing) in &objects {
        let desired = shapes
            .data
            .get(&phys.full_key)
            .map(|data| data.physics_shape_type);
        let scale = collider_extents(&phys.scale);
        let scale_changed = existing.is_none_or(|state| extents_differ(state.scale, scale));
        let shape_changed = existing.is_none_or(|state| state.shape != desired);
        // A geometry-needing shape whose collider is still the placeholder cuboid /
        // visual-geometry fallback: retry each frame until the meshes are uploaded
        // (or, for a mesh object, until its lighter physics shape is decoded).
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
                commands.entity(entity).insert(RefinedCollider {
                    collider: Some(SharedShape::cuboid(ex, ey, ez)),
                    shape: None,
                    from_geometry: false,
                    scale,
                });
            }
            // No collision shape: drop the collider (the mover keeps dead-reckoning).
            Some(PhysicsShapeType::None) => {
                debug!("physical object {entity} → no collider (PhysicsShapeType::None)");
                commands.entity(entity).insert(RefinedCollider {
                    collider: None,
                    shape: desired,
                    from_geometry: true,
                    scale,
                });
            }
            // Convex hull / exact prim / an unrecognised type.
            Some(shape) => {
                // A mesh object: prefer its uploaded physics shape — the analysed
                // collision hull the simulator uses, accurate and far lighter than the
                // visual mesh (the switch [[viewer-physics-static-prim-colliders]]
                // called for). Request it on demand and fall back to the visual
                // geometry until it decodes.
                let mesh_key = object_state
                    .static_collider_facts(&scene.scoped_id)
                    .and_then(|facts| facts.mesh);
                if let Some(mesh_key) = mesh_key {
                    mesh_manager.request_physics(mesh_key);
                    if let Some(collider) = mesh_manager
                        .physics(mesh_key)
                        .and_then(|physics| mesh_physics_collider(physics, scale))
                    {
                        debug!("physical object {entity} → {shape:?} collider from mesh physics");
                        commands.entity(entity).insert(RefinedCollider {
                            collider: Some(collider),
                            shape: desired,
                            from_geometry: true,
                            scale,
                        });
                        continue;
                    }
                }
                // Non-mesh (or a mesh whose physics is still fetching): build from the
                // object's own tessellated geometry.
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
                    let collider = if scale_changed || shape_changed {
                        Some(SharedShape::cuboid(ex, ey, ez))
                    } else {
                        existing.and_then(|state| state.collider.clone())
                    };
                    commands.entity(entity).insert(RefinedCollider {
                        collider,
                        shape: desired,
                        from_geometry: false,
                        scale,
                    });
                    continue;
                }
                let point_count = points.len();
                let collider = prim_geometry_collider(Some(shape), points, indices, scale);
                debug!("physical object {entity} → {shape:?} collider from {point_count} vertices");
                // A mesh on the visual fallback keeps `from_geometry: false` so it
                // retries for the lighter physics shape; a plain prim's tessellated
                // geometry is final (`true`).
                commands.entity(entity).insert(RefinedCollider {
                    collider: Some(collider),
                    shape: desired,
                    from_geometry: mesh_key.is_none(),
                    scale,
                });
            }
        }
    }
}

/// Scale a point cloud (mesh-local `[f32; 3]`) into an object entity's local frame
/// by the object scale — the frame its collision [`SharedShape`] lives in (the
/// entity carries no scale; that rides the geometry holder). Mirrors how
/// [`gather_object_geometry`] scales the visual vertices.
fn scaled_points(points: impl Iterator<Item = [f32; 3]>, scale: [f32; 3]) -> Vec<Vec3> {
    let [sx, sy, sz] = scale;
    points
        .map(|[x, y, z]| Vec3::new(x * sx, y * sy, z * sz))
        .collect()
}

/// Convert a Bevy point cloud into [`parry3d`]'s vector type. parry builds on its
/// own `glam` release, so the vector type does not unify with Bevy's — round-trip
/// through a plain array (cf. [`crate::raycast_index`]).
fn to_parry_points(points: &[Vec3]) -> Vec<ParryVec> {
    points
        .iter()
        .map(|point| ParryVec::from_array(point.to_array()))
        .collect()
}

/// Assemble a mesh's physics **triangle** submeshes into one scaled point cloud +
/// triangle index buffer (the exact-collision fallback when a mesh carries no
/// convex decomposition).
fn submesh_trimesh(submeshes: &[Submesh], scale: [f32; 3]) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let [sx, sy, sz] = scale;
    let mut points = Vec::new();
    let mut indices = Vec::new();
    for submesh in submeshes {
        let base = u32::try_from(points.len()).unwrap_or(u32::MAX);
        for &[x, y, z] in &submesh.positions {
            points.push(Vec3::new(x * sx, y * sy, z * sz));
        }
        for &[a, b, c] in submesh.indices.as_chunks::<3>().0 {
            indices.push([
                base.saturating_add(a),
                base.saturating_add(b),
                base.saturating_add(c),
            ]);
        }
    }
    (points, indices)
}

/// Build a [`parry3d`] [`SharedShape`] for a mesh object from its uploaded physics
/// shape ([`MeshPhysics`]), scaled by the object scale into the object entity's
/// local frame. Prefers the **convex-hull decomposition** (a compound of one
/// `convex_hull` per decomposed piece — the analysed collision shape the simulator
/// itself uses, concave-accurate yet far cheaper than the visual mesh); falls back
/// to the single low-detail **bounding hull**, then to the exact **physics triangle
/// mesh**. Returns `None` if the physics blocks held no usable geometry (the caller
/// then falls back to the visual geometry).
fn mesh_physics_collider(physics: &MeshPhysics, scale: [f32; 3]) -> Option<SharedShape> {
    if let Some(convex) = physics.convex.as_ref() {
        let hulls: Vec<(ParryPose, SharedShape)> = convex
            .hulls
            .iter()
            .filter_map(|hull| {
                let points = to_parry_points(&scaled_points(hull.iter().copied(), scale));
                SharedShape::convex_hull(&points).map(|shape| (ParryPose::identity(), shape))
            })
            .collect();
        if !hulls.is_empty() {
            return Some(SharedShape::compound(hulls));
        }
        // No per-piece hulls decoded: the single low-detail bounding hull is the
        // cheap broad-phase shape.
        if !convex.bounding_verts.is_empty() {
            let points =
                to_parry_points(&scaled_points(convex.bounding_verts.iter().copied(), scale));
            if let Some(shape) = SharedShape::convex_hull(&points) {
                return Some(shape);
            }
        }
    }
    // No convex block: the exact physics triangle mesh.
    if let Some(submeshes) = physics.mesh.as_ref() {
        let (points, indices) = submesh_trimesh(submeshes, scale);
        if !points.is_empty()
            && !indices.is_empty()
            && let Ok(shape) = SharedShape::trimesh(to_parry_points(&points), indices)
        {
            return Some(shape);
        }
    }
    None
}

/// Build a prim's [`parry3d`] [`SharedShape`] from its own tessellated geometry
/// (`points` already scaled into object-local space, `indices` its triangles): a
/// `convex_hull` when the physics shape is explicitly **convex hull**, else the
/// exact **trimesh** (the reference default for a legacy prim, and what a concave
/// prim — a hollow / an archway — needs so the camera passes through the gap). A
/// degenerate convex hull / trimesh falls back to a cuboid of `extents`.
fn prim_geometry_collider(
    shape: Option<PhysicsShapeType>,
    points: Vec<Vec3>,
    indices: Vec<[u32; 3]>,
    extents: [f32; 3],
) -> SharedShape {
    let [ex, ey, ez] = extents;
    let points = to_parry_points(&points);
    match shape {
        Some(PhysicsShapeType::ConvexHull) => {
            SharedShape::convex_hull(&points).unwrap_or_else(|| SharedShape::cuboid(ex, ey, ez))
        }
        _prim_none_or_unknown => SharedShape::trimesh(points, indices)
            .unwrap_or_else(|_| SharedShape::cuboid(ex, ey, ez)),
    }
}

/// The most static-index collider builds *gathered + spawned* in one frame. The
/// heavy collider construction runs off-thread ([`apply_static_colliders`]), but the
/// main-thread work this bounds — gathering each prim's vertices (copying them out of
/// the mesh assets) and spawning its task — still costs, and a region hand-off can
/// deliver hundreds of prims at once, so the work is drained a budget at a time over
/// subsequent frames (the crowd-spawn / asset-upload budget pattern). Colliders
/// missing for a frame or two is invisible; a stall is not. The budgeted prims are
/// chosen nearest-camera-first so the geometry the viewer is most likely to collide
/// with gets its collider soonest.
///
/// Kept modest (16) because a batch of collider builds landing at once still costs
/// main-thread geometry gathering + off-thread BVH construction; spreading the
/// builds thinner trades a slightly longer collider-stream-in for a flatter frame.
const STATIC_COLLIDER_BUDGET: usize = 16;

/// Records the static [`SharedShape`] collider a non-physical prim currently
/// carries (mirrored into the raycast index by [`sync_raycast_index`]), so
/// [`build_static_colliders`] rebuilds it only on a real change (a resize, a
/// phantom / physics-shape toggle, or the intended shape finally becoming available
/// after a placeholder). Its presence marks a prim already handled this session.
#[derive(Component)]
pub(crate) struct StaticCollider {
    /// The parry collision shape, in the prim's object-local frame (object scale
    /// baked in). Mirrored into [`RaycastIndexColliders`] each time it changes.
    collider: SharedShape,
    /// The object scale (floored extents, metres) the collider was built for.
    scale: [f32; 3],
    /// Whether the prim is indexed-only (phantom / physics-shape-`None`), so a
    /// phantom / physics-`None` toggle re-files it and the index marks it non-solid.
    non_solid: bool,
    /// The physics-shape type the collider was built for (`None` = unknown, the
    /// default), so a later `ObjectPhysicsProperties` push that changes it rebuilds.
    shape: Option<PhysicsShapeType>,
    /// `true` once the collider is the intended final shape (mesh physics for a mesh,
    /// tessellated geometry for a prim); `false` for a transient placeholder cuboid
    /// (a mesh awaiting its physics blocks, or geometry not yet uploaded), which is
    /// retried under the budget each frame until the real shape is in hand.
    settled: bool,
}

/// Whether a prim category gets a static-index collider. Plain prims, sculpts, and
/// meshes do; avatars are dynamic (and carry no collider by design — see
/// [`crate::camera::collide_camera`]); trees and grass are billboard / impostor
/// geometry whose holder is unscaled, so a collider from it would be wrong.
const fn category_gets_collider(category: ObjectCategory) -> bool {
    matches!(
        category,
        ObjectCategory::Prim | ObjectCategory::Sculpt | ObjectCategory::Mesh
    )
}

/// The shape source for one off-thread collider build, gathered on the main thread
/// (it needs ECS / asset access) and moved into an [`AsyncComputeTaskPool`] task
/// that turns it into a [`SharedShape`] — the expensive part (parry trimesh /
/// convex hull + its internal BVH) off the frame thread.
enum ColliderBuildJob {
    /// A mesh object's uploaded physics shape, scaled by the object scale.
    MeshPhysics(Arc<MeshPhysics>, [f32; 3]),
    /// A prim's tessellated geometry (points already scaled into object-local
    /// space), built as a `convex_hull` / `trimesh` per its physics shape.
    Geometry {
        /// The scaled vertex point cloud.
        points: Vec<Vec3>,
        /// The triangle index buffer.
        indices: Vec<[u32; 3]>,
        /// The physics shape (convex vs trimesh).
        shape: Option<PhysicsShapeType>,
        /// The cuboid extents used if a convex hull comes out degenerate.
        extents: [f32; 3],
    },
}

/// Construct the [`SharedShape`] for a [`ColliderBuildJob`] — the CPU-heavy work run
/// on an [`AsyncComputeTaskPool`] thread. A mesh whose physics blocks turn out to
/// hold no usable geometry falls back to a cuboid of its scale.
fn run_collider_build(job: ColliderBuildJob) -> SharedShape {
    match job {
        ColliderBuildJob::MeshPhysics(physics, scale) => mesh_physics_collider(&physics, scale)
            .unwrap_or_else(|| {
                let [x, y, z] = scale;
                SharedShape::cuboid(x, y, z)
            }),
        ColliderBuildJob::Geometry {
            points,
            indices,
            shape,
            extents,
        } => prim_geometry_collider(shape, points, indices, extents),
    }
}

/// The static-index collider record to attach once its off-thread build finishes,
/// paired with the running [`Task`] in [`StaticColliderBuilds`].
struct StaticBuildTask {
    /// The running collider construction.
    task: Task<SharedShape>,
    /// The object scale the collider is being built for.
    scale: [f32; 3],
    /// Whether the prim is indexed-only (phantom / physics-shape-`None`), so the
    /// raycast index files it as non-solid.
    non_solid: bool,
    /// The physics shape the collider is being built for.
    shape: Option<PhysicsShapeType>,
    /// Whether this is the intended final shape (vs a mesh's visual-geometry fallback
    /// awaiting its lighter physics shape, which is retried).
    settled: bool,
}

/// The in-flight off-thread static-collider builds, keyed by prim entity, so the
/// scanner ([`build_static_colliders`]) does not re-queue a prim whose collider is
/// already building and [`apply_static_colliders`] can install each finished one.
#[derive(Resource, Default)]
pub(crate) struct StaticColliderBuilds {
    /// One running build per prim entity.
    tasks: HashMap<Entity, StaticBuildTask>,
}

/// One prim needing a static-index collider built this frame, with the world
/// position used to rank it by camera proximity.
struct ColliderWork {
    /// The prim entity to attach the collider to.
    entity: Entity,
    /// The prim's world position (Bevy), for the nearest-camera-first ordering.
    world_pos: Vec3,
    /// The mesh asset key when the prim is a mesh (its collider comes from the mesh
    /// physics shape), else `None` (built from tessellated geometry).
    mesh: Option<MeshKey>,
    /// The floored collider extents (object scale) — the placeholder cuboid size and
    /// the geometry scale.
    scale: [f32; 3],
    /// The physics-shape type, if known (drives convex vs trimesh; `Some(None)`
    /// files the prim as non-solid).
    shape: Option<PhysicsShapeType>,
    /// Whether the prim is indexed-only (phantom / physics-shape-`None`), so the
    /// raycast index files it as non-solid.
    non_solid: bool,
    /// Whether the prim already carries a collider (a placeholder or a stale shape),
    /// so a still-pending prim is not re-inserted with an identical placeholder each
    /// frame (which would dirty its transform subtree).
    has_collider: bool,
}

/// Give **every** non-physical, non-avatar, non-attachment prim a static
/// [`SharedShape`] collider (marked with [`StaticCollider`]), which
/// [`sync_raycast_index`] mirrors into the custom [`crate::raycast_index`] index —
/// the shared scene index that makes [`crate::camera::collide_camera`] functional
/// ([[viewer-perf-custom-static-raycast-index]]). Physical roots keep their
/// collider (`refine_physical_colliders`); this handles all the other solid
/// world geometry — walls, floors, buildings, linkset children.
///
/// A mesh prim's collider comes from its uploaded physics shape ([`MeshPhysics`],
/// requested on demand); a plain prim / sculpt from its tessellated geometry. A
/// phantom prim or a physics-shape-`None` prim still gets a collider (so it is in
/// the index) but is marked non-solid, which physics-collision queries skip (the
/// camera, visual occlusion, uses all colliders).
///
/// Colliders are built lazily once the geometry (or mesh physics) is available and
/// **budgeted** ([`STATIC_COLLIDER_BUDGET`] gathers per frame, nearest-camera-first)
/// so a region hand-off streams them in over several frames instead of spiking one.
/// The CPU-heavy collider construction itself runs **off the frame thread** on an
/// [`AsyncComputeTaskPool`]; this system only gathers the shape source (which needs
/// asset access) and spawns the task, and [`apply_static_colliders`] installs the
/// finished collider. A prim with no geometry yet gets a cheap placeholder cuboid
/// (built inline — O(1)) and is retried.
#[expect(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "an ECS system's arguments are its injected queries / resources"
)]
pub(crate) fn build_static_colliders(
    object_state: Res<ObjectState>,
    shapes: Res<ObjectPhysicsShapes>,
    mut mesh_manager: ResMut<MeshManager>,
    meshes: Res<Assets<Mesh>>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    prims: Query<
        (
            Entity,
            &SceneObject,
            &ObjectSlMotion,
            &GlobalTransform,
            Option<&StaticCollider>,
        ),
        Without<PhysicalObject>,
    >,
    children_q: Query<&Children>,
    holders: Query<(), With<GeometryHolder>>,
    mesh_handles: Query<&Mesh3d>,
    mut builds: ResMut<StaticColliderBuilds>,
    mut commands: Commands,
) {
    // Gather the prims whose static collider is missing / stale, each tagged with
    // the facts and the world position its proximity ranking needs. A prim whose
    // build is already in flight is skipped (it is not re-queued until it lands).
    let mut work: Vec<ColliderWork> = Vec::new();
    for (entity, scene, sl_motion, global, existing) in &prims {
        let facts = object_state.static_collider_facts(&scene.scoped_id);
        // Whether this prim should carry a static-index collider at all: a plain
        // prim / sculpt / mesh, not worn, not flexi, and tracked.
        let disqualified = !category_gets_collider(scene.category)
            || sl_motion.attachment
            || facts.as_ref().is_none_or(|facts| facts.flexi);
        if disqualified {
            // Remove any collider it acquired **earlier**, then move on. A worn mesh
            // (a BoM body, rigged clothing) can stream in as a plain object *before*
            // its attachment point is known — so it briefly looks like an in-world
            // prim and gets a collider near the avatar. Once it is recognised as an
            // attachment the scanner would otherwise just skip it, leaving that stale
            // collider parked on the avatar to yank the third-person camera into the
            // head. Removing it (and cancelling any in-flight build) is the fix.
            if existing.is_some() {
                commands.entity(entity).remove::<StaticCollider>();
            }
            let _cancelled = builds.tasks.remove(&entity);
            continue;
        }
        if builds.tasks.contains_key(&entity) {
            // Build already in flight: not re-queued until it lands.
            continue;
        }
        let Some(facts) = facts else {
            // Unreachable: `disqualified` is true when facts is `None`.
            continue;
        };
        let scale = collider_extents(&sl_motion.scale);
        let shape = shapes
            .data
            .get(&facts.full_key)
            .map(|data| data.physics_shape_type);
        let non_solid = facts.phantom || shape == Some(PhysicsShapeType::None);
        // Skip prims whose collider is already the intended shape at the current
        // scale / layer / shape — the steady-state majority.
        let needs_build = existing.is_none_or(|state| {
            !state.settled
                || extents_differ(state.scale, scale)
                || state.non_solid != non_solid
                || state.shape != shape
        });
        if !needs_build {
            continue;
        }
        work.push(ColliderWork {
            entity,
            world_pos: global.translation(),
            mesh: facts.mesh,
            scale,
            shape,
            non_solid,
            has_collider: existing.is_some(),
        });
    }
    if work.is_empty() {
        return;
    }
    // Nearest-camera-first, so the geometry the viewer is most likely to collide
    // with gets its collider soonest. A missing camera leaves the (arbitrary) query
    // order.
    if let Ok(camera) = camera.single() {
        let eye = camera.translation();
        work.sort_by(|a, b| {
            a.world_pos
                .distance_squared(eye)
                .total_cmp(&b.world_pos.distance_squared(eye))
        });
    }

    for item in work.into_iter().take(STATIC_COLLIDER_BUDGET) {
        // Gather the shape source on the main thread (asset access), deciding the
        // intended shape vs a not-ready placeholder; `None` job = geometry / physics
        // not available yet.
        let (job, settled): (Option<ColliderBuildJob>, bool) = match item.mesh {
            Some(mesh_key) => {
                // Trigger the on-demand physics-block fetch for this (near-camera)
                // mesh; use it once decoded, else fall back to the visual geometry.
                mesh_manager.request_physics(mesh_key);
                if let Some(physics) = mesh_manager.physics(mesh_key) {
                    (
                        Some(ColliderBuildJob::MeshPhysics(
                            Arc::clone(physics),
                            item.scale,
                        )),
                        true,
                    )
                } else {
                    let (points, indices) = gather_object_geometry(
                        item.entity,
                        item.scale,
                        &children_q,
                        &holders,
                        &mesh_handles,
                        &meshes,
                    );
                    if points.is_empty() {
                        (None, false)
                    } else {
                        // A valid (heavier) fallback while the physics fetches; keep
                        // `settled = false` so it retries for the lighter shape.
                        (
                            Some(ColliderBuildJob::Geometry {
                                points,
                                indices,
                                shape: item.shape,
                                extents: item.scale,
                            }),
                            false,
                        )
                    }
                }
            }
            None => {
                let (points, indices) = gather_object_geometry(
                    item.entity,
                    item.scale,
                    &children_q,
                    &holders,
                    &mesh_handles,
                    &meshes,
                );
                if points.is_empty() {
                    (None, false)
                } else {
                    (
                        Some(ColliderBuildJob::Geometry {
                            points,
                            indices,
                            shape: item.shape,
                            extents: item.scale,
                        }),
                        true,
                    )
                }
            }
        };

        match job {
            // Neither geometry nor mesh physics ready: install a cheap placeholder
            // cuboid (only the first time, to avoid re-dirtying it every retry) and
            // try again next frame.
            None => {
                if !item.has_collider {
                    let [ex, ey, ez] = item.scale;
                    commands.entity(item.entity).insert(StaticCollider {
                        collider: SharedShape::cuboid(ex, ey, ez),
                        scale: item.scale,
                        non_solid: item.non_solid,
                        shape: item.shape,
                        settled: false,
                    });
                }
            }
            // Build the real shape off-thread; `apply_static_colliders` installs it.
            Some(job) => {
                let task =
                    AsyncComputeTaskPool::get().spawn(async move { run_collider_build(job) });
                builds.tasks.insert(
                    item.entity,
                    StaticBuildTask {
                        task,
                        scale: item.scale,
                        non_solid: item.non_solid,
                        shape: item.shape,
                        settled,
                    },
                );
            }
        }
    }
}

/// Install each finished off-thread static-collider build ([`build_static_colliders`]):
/// poll the in-flight tasks, and for each that completed attach its
/// [`StaticCollider`] record (which carries the built [`SharedShape`];
/// [`sync_raycast_index`] then mirrors it into the index) — unless the prim has
/// since been despawned or become a physical root (the physical path owns its
/// collider then), in which case the built shape is simply dropped.
pub(crate) fn apply_static_colliders(
    mut builds: ResMut<StaticColliderBuilds>,
    physical: Query<(), With<PhysicalObject>>,
    mut commands: Commands,
) {
    let mut finished: Vec<(Entity, SharedShape)> = Vec::new();
    for (&entity, build) in &mut builds.tasks {
        if let Some(collider) = block_on(poll_once(&mut build.task)) {
            finished.push((entity, collider));
        }
    }
    for (entity, collider) in finished {
        let Some(build) = builds.tasks.remove(&entity) else {
            continue;
        };
        // The prim became physical while its build ran: drop the collider (the
        // physical path owns it now).
        if physical.get(entity).is_ok() {
            continue;
        }
        // The prim may have been despawned; only install onto a live entity.
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.insert(StaticCollider {
                collider,
                scale: build.scale,
                non_solid: build.non_solid,
                shape: build.shape,
                settled: build.settled,
            });
        }
    }
}

/// Strip the static-index collider from a prim that has become a **physical** root
/// (it gained a [`PhysicalObject`] marker): the physical path
/// (`drive_physical_objects` / `refine_physical_colliders`) now owns its
/// collider, and a leftover [`StaticCollider`] would both fight it and — once the
/// prim went non-physical again — wrongly mark it "already handled". Removing it
/// also drops it from the raycast index on the next [`sync_raycast_index`] pass,
/// and lets [`build_static_colliders`] rebuild a static collider if it reverts. Any
/// in-flight off-thread build is dropped by [`apply_static_colliders`] when it lands.
pub(crate) fn detach_static_colliders(
    now_physical: Query<Entity, (With<StaticCollider>, With<PhysicalObject>)>,
    mut commands: Commands,
) {
    for entity in &now_physical {
        commands.entity(entity).remove::<StaticCollider>();
    }
}

/// Mirror the static-index colliders into the custom [`RaycastIndexColliders`]
/// set (the [`crate::raycast_index`] replacement for avian's `SpatialQuery`).
///
/// Change-driven: a collider is (re-)inserted only when it is freshly installed
/// ([`Added<StaticCollider>`]), rebuilt (a resize / shape change re-inserts
/// [`StaticCollider`], so [`Changed<StaticCollider>`] fires), or the prim's world
/// pose moves ([`Changed<GlobalTransform>`], e.g. a region-origin rebase), and
/// removed when its [`StaticCollider`] goes away ([`detach_static_colliders`] /
/// despawn). The prim's object scale is baked into the collider geometry, so the
/// index pose is the entity's world translation + rotation only. `solid` follows
/// the prim's collidability (`!non_solid`), though camera collision — the sole
/// consumer today — queries all colliders regardless.
#[expect(
    clippy::type_complexity,
    reason = "an ECS system's arguments are its injected queries; the change-detection filter is inherently a nested tuple"
)]
pub(crate) fn sync_raycast_index(
    mut index: ResMut<RaycastIndexColliders>,
    changed: Query<
        (Entity, &GlobalTransform, &StaticCollider),
        Or<(
            Added<StaticCollider>,
            Changed<StaticCollider>,
            Changed<GlobalTransform>,
        )>,
    >,
    mut removed: RemovedComponents<StaticCollider>,
) {
    for entity in removed.read() {
        index.remove(entity);
    }
    for (entity, global, static_collider) in &changed {
        let (_scale, rotation, translation) = global.to_scale_rotation_translation();
        index.upsert(
            entity,
            static_collider.collider.clone(),
            translation,
            rotation,
            !static_collider.non_solid,
        );
    }
}

/// Refill the moving-collider set ([`DynamicColliders`]) from the physical prims'
/// current world poses, each frame. Physical prims move continuously, so — unlike
/// the static BVH — they are kept in a small linear set that never triggers an
/// off-thread rebuild. A prim contributes only once its [`RefinedCollider`] holds
/// a shape (a `PhysicsShapeType::None` prim has no collider and is skipped, so it
/// neither blocks the camera nor makes collision sounds).
pub(crate) fn sync_dynamic_colliders(
    mut dynamic: ResMut<DynamicColliders>,
    physical: Query<(Entity, &GlobalTransform, &RefinedCollider), With<PhysicsInterp>>,
) {
    dynamic.clear();
    for (entity, global, refined) in &physical {
        let Some(shape) = refined.collider.as_ref() else {
            continue;
        };
        let (_scale, rotation, translation) = global.to_scale_rotation_translation();
        dynamic.push(entity, shape.clone(), translation, rotation, true);
    }
}

/// Whether `SL_VIEWER_LOG_CAMERA_COLLISION=1` is set — the diagnostic that hunts a
/// stray collider the third-person camera pulls in on (a collider sitting on / near
/// the avatar). Read once.
fn log_camera_collision_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("SL_VIEWER_LOG_CAMERA_COLLISION")
            .is_ok_and(|value| value == "1" || value == "true")
    })
}

/// `SL_VIEWER_LOG_CAMERA_COLLISION=1`: once a second, log every collidered prim
/// within 6 m of the own avatar's body-root focus — its distance and identity
/// (object category, attachment flag, and which path owns the collider: the static
/// index or the physical-prim path) — so a collider the camera wrongly pulls in on
/// (the "eye stuck in the head" report) can be identified. Silent by default; a
/// pure read (no mutation).
#[expect(
    clippy::type_complexity,
    reason = "an ECS system's arguments are its injected queries / resources"
)]
pub(crate) fn log_colliders_near_avatar(
    time: Res<Time>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    object_state: Res<ObjectState>,
    transforms: Query<&GlobalTransform>,
    colliders: Query<
        (
            Entity,
            &GlobalTransform,
            Option<&SceneObject>,
            Option<&ObjectSlMotion>,
            Has<StaticCollider>,
            Has<PhysicalObject>,
            Has<AvatarMotion>,
        ),
        Or<(With<StaticCollider>, With<RefinedCollider>)>,
    >,
    mut last: Local<f64>,
) {
    if !log_camera_collision_enabled() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if now - *last < 1.0 {
        return;
    }
    let Some(agent) = identity.agent_id else {
        return;
    };
    let Some(anchor) = avatars.body_root_of(agent) else {
        return;
    };
    let Ok(anchor_transform) = transforms.get(anchor) else {
        return;
    };
    *last = now;
    let focus = anchor_transform.translation();
    let mut near: Vec<(
        Entity,
        f32,
        Vec3,
        Option<&SceneObject>,
        Option<&ObjectSlMotion>,
        bool,
        bool,
        bool,
    )> = colliders
        .iter()
        .map(
            |(entity, global, scene, sl_motion, is_static, is_physical, is_avatar)| {
                let pos = global.translation();
                (
                    entity,
                    focus.distance(pos),
                    pos,
                    scene,
                    sl_motion,
                    is_static,
                    is_physical,
                    is_avatar,
                )
            },
        )
        .filter(|entry| entry.1 < 6.0)
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1));
    info!(
        "camera-collision: {} colliders within 6m of avatar focus {focus:?}",
        near.len()
    );
    for (entity, dist, pos, scene, sl_motion, is_static, is_physical, is_avatar) in
        near.into_iter().take(24)
    {
        let category = scene.map(|scene| scene.category);
        let attachment = sl_motion.is_some_and(|motion| motion.attachment);
        let mesh = scene.and_then(|scene| {
            object_state
                .static_collider_facts(&scene.scoped_id)
                .and_then(|facts| facts.mesh)
        });
        info!(
            "  collider {entity} d={dist:.2}m pos={pos:?} category={category:?} \
             attachment={attachment} static={is_static} physical={is_physical} \
             avatar={is_avatar} mesh={mesh:?}"
        );
    }
}

/// Whether this avatar's authoritative position sits at (or within `margin`
/// metres above) the **stricter avatar ground floor** (`avatar_ground_floor`:
/// `land + 0.5 * height`) for the terrain beneath it — i.e. the avatar is on /
/// very close to the ground rather than up in the air. The viewer's movement
/// controls ([`crate::movement`]) use this to auto-stop flying on landing
/// (P31.11). Returns `false` when the land height under the avatar is not yet
/// known (terrain not ingested), so an unknown floor never forces a landing.
#[must_use]
pub(crate) fn avatar_at_ground_floor(
    motion: &AvatarMotion,
    terrain: &TerrainState,
    margin: f32,
) -> bool {
    avatar_ground_floor(
        terrain.land_height(motion.region_handle, motion.position.x, motion.position.y),
        motion.height,
    )
    .is_some_and(|floor| motion.position.z <= floor + margin)
}

#[cfg(test)]
mod tests {
    use super::{
        ClampInput, MAX_INTERP_SECS, MotionState, OBJECT_SMOOTHING_TAU_SECS, PHASE_OUT_START_SECS,
        PhysicsInterp, REGION_MAX_HEIGHT_M, REGION_WIDTH_M, ROTATION_SMOOTHING_TAU_SECS,
        TRANSLATION_SNAP_DISTANCE_M, advance_motion, angular_step, append_triangles,
        avatar_collision_floor, avatar_ground_floor, bevy_position_of, bevy_rotation_of,
        category_gets_collider, clamp_dilation, clamp_prediction, dead_reckon, eased_translation,
        extents_differ, ground_floor, mesh_physics_collider, neighbours_known, phase_out_factor,
        place_smoothed, prim_geometry_collider, reaim_residual, rotation_smoothing_alpha,
        shape_wants_geometry, smoothing_alpha, submesh_trimesh, to_parry_points,
    };
    use crate::objects::ObjectCategory;
    use crate::physics::RegionTimeDilation;
    use bevy::math::{Quat, Vec3};
    use bevy::mesh::Indices;
    use bevy::transform::components::Transform;
    use parry3d::math::Pose as ParryPose;
    use parry3d::shape::SharedShape;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        MeshPhysics, PhysicsConvex, PhysicsShapeType, RegionHandle, Rotation, Submesh, Vector,
    };

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

    /// A healthy region (dilation `1.0`) runs the dead-reckoning step at full
    /// speed; a laden one scales it down; the endpoints pass through unchanged.
    #[test]
    fn dilation_clamps_into_the_relative_speed_domain() {
        approx(clamp_dilation(1.0), 1.0);
        approx(clamp_dilation(0.5), 0.5);
        approx(clamp_dilation(0.0), 0.0);
    }

    /// An out-of-range or non-finite dilation can never poison the prediction: it
    /// is clamped into range, and a `NaN` falls back to full speed.
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
        let mid = f64::midpoint(PHASE_OUT_START_SECS, MAX_INTERP_SECS);
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
            SharedShape::convex_hull(&to_parry_points(&corners)).is_some(),
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
        let built = SharedShape::trimesh(to_parry_points(&vertices), vec![[0, 1, 2], [0, 2, 3]]);
        assert!(
            built.is_ok_and(|collider| {
                let aabb = collider.compute_aabb(&ParryPose::identity());
                (aabb.maxs.x - 2.0).abs() < 1.0e-4 && (aabb.maxs.y - 2.0).abs() < 1.0e-4
            }),
            "trimesh aabb should span the 2x2 quad"
        );
    }

    /// The eight corners of a unit cube, centred on the origin.
    fn cube_corners() -> Vec<[f32; 3]> {
        vec![
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
        ]
    }

    /// A mesh's convex-hull decomposition builds a compound collider, and its points
    /// are scaled by the object scale into the object-local frame (a unit cube at
    /// scale `[2, 4, 6]` spans that box).
    #[test]
    fn mesh_physics_collider_uses_scaled_convex_decomposition() {
        let physics = MeshPhysics {
            convex: Some(PhysicsConvex {
                hulls: vec![cube_corners()],
                ..PhysicsConvex::default()
            }),
            mesh: None,
        };
        let built = mesh_physics_collider(&physics, [2.0, 4.0, 6.0]);
        assert!(
            built.is_some_and(|collider| {
                let aabb = collider.compute_aabb(&ParryPose::identity());
                (aabb.maxs.x - 1.0).abs() < 1.0e-4
                    && (aabb.maxs.y - 2.0).abs() < 1.0e-4
                    && (aabb.maxs.z - 3.0).abs() < 1.0e-4
            }),
            "a scaled unit cube's convex hull spans half-extents [1, 2, 3]"
        );
    }

    /// With no per-piece hulls, the single low-detail bounding hull is used.
    #[test]
    fn mesh_physics_collider_falls_back_to_bounding_hull() {
        let physics = MeshPhysics {
            convex: Some(PhysicsConvex {
                hulls: Vec::new(),
                bounding_verts: cube_corners(),
                ..PhysicsConvex::default()
            }),
            mesh: None,
        };
        assert!(
            mesh_physics_collider(&physics, [1.0, 1.0, 1.0]).is_some(),
            "the bounding hull should build a collider when no decomposition is present"
        );
    }

    /// With no convex block at all, the exact physics triangle mesh is used.
    #[test]
    fn mesh_physics_collider_uses_physics_trimesh() {
        let submesh = Submesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            indices: vec![0, 1, 2],
            ..Submesh::default()
        };
        let physics = MeshPhysics {
            convex: None,
            mesh: Some(vec![submesh]),
        };
        assert!(
            mesh_physics_collider(&physics, [1.0, 1.0, 1.0]).is_some(),
            "the physics triangle mesh should build a collider"
        );
    }

    /// A mesh with no physics blocks yields no collider (the caller then falls back
    /// to the visual geometry).
    #[test]
    fn mesh_physics_collider_none_when_empty() {
        assert!(
            mesh_physics_collider(&MeshPhysics::default(), [1.0, 1.0, 1.0]).is_none(),
            "no physics blocks means no physics collider"
        );
    }

    /// Two physics submeshes combine into one trimesh: the second's indices are
    /// offset past the first's vertices, and every point is scaled by the object
    /// scale.
    #[test]
    fn submesh_trimesh_offsets_indices_and_scales() {
        let a = Submesh {
            positions: vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            indices: vec![0, 1, 2],
            ..Submesh::default()
        };
        let b = Submesh {
            positions: vec![[1.0, 1.0, 1.0], [2.0, 2.0, 2.0], [3.0, 3.0, 3.0]],
            indices: vec![0, 1, 2],
            ..Submesh::default()
        };
        let (points, indices) = submesh_trimesh(&[a, b], [2.0, 2.0, 2.0]);
        assert_eq!(points.len(), 6);
        // First point scaled by 2.
        assert!(
            points
                .first()
                .is_some_and(|p| p.abs_diff_eq(Vec3::new(2.0, 0.0, 0.0), 1.0e-6))
        );
        // The second submesh's triangle references vertices 3/4/5.
        assert_eq!(indices, vec![[0, 1, 2], [3, 4, 5]]);
    }

    /// The convex-hull physics shape builds a convex collider; anything else (the
    /// exact prim, an unknown type) builds the trimesh, so a concave prim keeps its
    /// hole.
    #[test]
    fn prim_geometry_collider_picks_convex_or_trimesh() {
        let points = cube_corners().into_iter().map(Vec3::from).collect();
        let convex = prim_geometry_collider(
            Some(PhysicsShapeType::ConvexHull),
            points,
            Vec::new(),
            [1.0, 1.0, 1.0],
        );
        assert!(
            convex.compute_aabb(&ParryPose::identity()).maxs.x > 0.0,
            "the convex hull collider has a positive extent"
        );
        let quad = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
        ];
        let trimesh = prim_geometry_collider(
            Some(PhysicsShapeType::Prim),
            quad,
            vec![[0, 1, 2]],
            [1.0, 1.0, 1.0],
        );
        let aabb = trimesh.compute_aabb(&ParryPose::identity());
        assert!(
            (aabb.maxs.x - 2.0).abs() < 1.0e-4,
            "the exact-prim trimesh spans its geometry, got {aabb:?}"
        );
    }

    /// Only plain prims, sculpts, and meshes get a static-index collider; avatars,
    /// trees, and grass do not.
    #[test]
    fn category_gets_collider_selects_solid_prim_kinds() {
        assert!(category_gets_collider(ObjectCategory::Prim));
        assert!(category_gets_collider(ObjectCategory::Sculpt));
        assert!(category_gets_collider(ObjectCategory::Mesh));
        assert!(!category_gets_collider(ObjectCategory::Avatar));
        assert!(!category_gets_collider(ObjectCategory::Tree));
        assert!(!category_gets_collider(ObjectCategory::Grass));
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

    #[test]
    fn collision_floor_takes_the_higher_of_land_and_plane() {
        // Flat plane at Z = 25 above land 20: the plane wins, +0.5·height 2 = 26.
        let plane = Some([0.0, 0.0, 1.0, 25.0]);
        let floor = avatar_collision_floor(plane, Some(20.0), 128.0, 128.0, 2.0);
        assert!(
            floor.is_some_and(|floor| (floor - 26.0).abs() <= 1.0e-4),
            "plane above land should floor at 26, got {floor:?}"
        );
        // Plane below the land: the land wins (21).
        let low_plane = Some([0.0, 0.0, 1.0, 18.0]);
        let floor = avatar_collision_floor(low_plane, Some(20.0), 128.0, 128.0, 2.0);
        assert!(
            floor.is_some_and(|floor| (floor - 21.0).abs() <= 1.0e-4),
            "plane below land should floor at the land 21, got {floor:?}"
        );
    }

    #[test]
    fn collision_floor_uses_the_plane_when_land_is_unknown() {
        // Terrain patches mid-rebuild → no land height. The plane still floors the
        // avatar (Z = 30, +0.5·height 2 = 31), so a bounce cannot sink it.
        let plane = Some([0.0, 0.0, 1.0, 30.0]);
        let floor = avatar_collision_floor(plane, None, 128.0, 128.0, 2.0);
        assert!(
            floor.is_some_and(|floor| (floor - 31.0).abs() <= 1.0e-4),
            "plane-only floor should be 31, got {floor:?}"
        );
        // Neither → no floor (airborne over an un-ingested region).
        assert!(avatar_collision_floor(None, None, 128.0, 128.0, 2.0).is_none());
        // A near-vertical plane is ignored; with no land there is no floor.
        let vertical = Some([1.0, 0.0, 0.0, 5.0]);
        assert!(avatar_collision_floor(vertical, None, 128.0, 128.0, 2.0).is_none());
    }

    /// A zero-length rotation. The identity Second Life quaternion, for seeding a
    /// `MotionState` whose orientation should stay put.
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

    /// A test region handle shared by the object-residual tests.
    fn test_region() -> RegionHandle {
        RegionHandle::from_global(1000 * 256, 1000 * 256)
    }

    /// Build a [`PhysicsInterp`] whose prediction sits at a Second Life region-local
    /// position with the identity facing and a zero residual (rendered exactly at
    /// truth) — the starting point for the object-easing tests.
    fn interp_at(position: [f32; 3]) -> PhysicsInterp {
        let [x, y, z] = position;
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        PhysicsInterp {
            motion: MotionState::new(
                &Vector { x, y, z },
                &zero,
                &zero,
                &identity_rotation(),
                &zero,
                test_region(),
            ),
            last_message_secs: 0.0,
            last_interp_secs: 0.0,
            collider_scale: [1.0, 1.0, 1.0],
            render_offset: Vec3::ZERO,
            render_rot_offset: Quat::IDENTITY,
            rest: None,
        }
    }

    /// Re-seed an interp's prediction to a fresh authoritative position (the reseed a
    /// new `ObjectUpdate` performs), leaving the residual for [`reaim_residual`].
    fn reseed_to(interp: &mut PhysicsInterp, position: [f32; 3]) {
        let [x, y, z] = position;
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        interp.motion = MotionState::new(
            &Vector { x, y, z },
            &zero,
            &zero,
            &identity_rotation(),
            &zero,
            test_region(),
        );
    }

    /// The rendered position `predicted + residual` (the pose [`place_smoothed`] writes
    /// before decaying the residual).
    fn rendered_of(interp: &PhysicsInterp) -> Vec3 {
        let p = bevy_position_of(&interp.motion);
        Vec3::new(
            p.x + interp.render_offset.x,
            p.y + interp.render_offset.y,
            p.z + interp.render_offset.z,
        )
    }

    /// The generic exponential-smoothing blend is `1` for a non-positive frame and
    /// reaches ~63 % at one time constant, for any tau (the object path uses its own
    /// [`OBJECT_SMOOTHING_TAU_SECS`], distinct from the avatar rotation tau).
    #[test]
    fn smoothing_alpha_reaches_63pct_at_one_tau() {
        near(smoothing_alpha(0.0, OBJECT_SMOOTHING_TAU_SECS), 1.0);
        near(smoothing_alpha(-1.0, OBJECT_SMOOTHING_TAU_SECS), 1.0);
        near(
            smoothing_alpha(OBJECT_SMOOTHING_TAU_SECS, OBJECT_SMOOTHING_TAU_SECS),
            1.0 - core::f32::consts::E.recip(),
        );
    }

    /// A divergent authoritative update does not snap the rendered object: after
    /// re-seeding to the new truth, [`reaim_residual`] sets the residual so the rendered
    /// pose (`predicted + residual`) stays exactly where it was last frame — the object
    /// rubberband fix. The residual then eases to zero, converging the render onto truth.
    #[test]
    fn reaim_residual_keeps_render_continuous_then_eases_to_truth() {
        let mut interp = interp_at([10.0, 20.0, 30.0]);
        // Rendered last frame exactly at the old prediction (zero residual).
        let rendered_before = bevy_position_of(&interp.motion);
        let rendered_rot_before = bevy_rotation_of(&interp.motion);
        // A new update corrects the position by 1 m (prediction diverged from truth).
        reseed_to(&mut interp, [11.0, 20.0, 30.0]);
        reaim_residual(&mut interp, rendered_before, rendered_rot_before);
        // The rendered pose is unchanged across the reseed (no visible snap).
        let rendered = rendered_of(&interp);
        near3(
            [rendered.x, rendered.y, rendered.z],
            [rendered_before.x, rendered_before.y, rendered_before.z],
        );
        // Easing many frames drives the residual to zero — the render converges to truth.
        let mut transform = Transform::IDENTITY;
        for _ in 0..300 {
            place_smoothed(&mut interp, &mut transform, 0.016, Vec3::ZERO);
        }
        let truth = bevy_position_of(&interp.motion);
        assert!(
            transform.translation.distance(truth) < 1.0e-3,
            "render should converge onto truth, got {:?} vs {truth:?}",
            transform.translation
        );
        assert!(interp.render_offset.length() < 1.0e-3);
    }

    /// A region-scale gap (a crossing / rebase / teleport) is snapped, not eased:
    /// [`reaim_residual`] zeroes the residual so the object renders at truth immediately
    /// rather than sliding hundreds of metres across the region.
    #[test]
    fn reaim_residual_snaps_a_region_scale_gap() {
        let mut interp = interp_at([250.0, 20.0, 30.0]);
        let rendered_before = bevy_position_of(&interp.motion);
        let rendered_rot_before = bevy_rotation_of(&interp.motion);
        // The object crosses a region border: its region-local X wraps ~245 m, well
        // past [`OBJECT_SNAP_DISTANCE_M`].
        reseed_to(&mut interp, [5.0, 20.0, 30.0]);
        reaim_residual(&mut interp, rendered_before, rendered_rot_before);
        assert!(
            interp.render_offset.length() < f32::EPSILON,
            "a region-scale gap should snap (zero residual), got {:?}",
            interp.render_offset
        );
        // The rendered pose is truth, not the pre-crossing position.
        let truth = bevy_position_of(&interp.motion);
        assert!(rendered_of(&interp).distance(truth) < 1.0e-4);
    }

    /// An ordinary per-update prediction correction eases: the next rendered
    /// position lies strictly between the last rendered position and the fresh
    /// authoritative truth, so a terse-update correction never hard-snaps.
    #[test]
    fn eased_translation_eases_a_small_correction() {
        let rendered = Vec3::new(10.0, 5.0, -3.0);
        let truth = Vec3::new(10.6, 5.0, -3.0);
        let next = eased_translation(rendered, truth, false, 0.3);
        assert!(
            next.x > rendered.x && next.x < truth.x,
            "eased toward truth, not snapped"
        );
        // 30 % of the 0.6 m gap.
        near(next.x, 10.18);
    }

    /// A region crossing snaps regardless of how small the numeric jump looks,
    /// so the 256 m rebase never glides across.
    #[test]
    fn eased_translation_snaps_on_region_crossing() {
        let rendered = Vec3::new(10.0, 5.0, -3.0);
        let truth = Vec3::new(10.2, 5.0, -3.0);
        let next = eased_translation(rendered, truth, true, 0.3);
        assert!(next.distance(truth) < 1.0e-6, "a crossing snaps to truth");
    }

    /// A region-scale jump (a teleport, or a rebase that read as a huge delta)
    /// snaps even without the region-crossing flag.
    #[test]
    fn eased_translation_snaps_a_region_scale_jump() {
        let rendered = Vec3::new(0.0, 0.0, 0.0);
        let truth = Vec3::new(TRANSLATION_SNAP_DISTANCE_M + 10.0, 0.0, 0.0);
        let next = eased_translation(rendered, truth, false, 0.3);
        assert!(
            next.distance(truth) < 1.0e-6,
            "a region-scale jump snaps to truth"
        );
    }

    /// A near-target ease converges *exactly* (the terminal snap): the asymptotic
    /// lerp alone can stall a hair short of the target in `f32`, which would keep
    /// the anchor `Transform` marked changed forever.
    #[test]
    fn eased_translation_settles_exactly_near_the_target() {
        let truth = Vec3::new(12.0, 3.0, -7.0);
        let rendered = Vec3::new(12.000_05, 3.0, -7.0);
        let next = eased_translation(rendered, truth, false, 0.3);
        assert_eq!(next, truth, "a sub-epsilon residual snaps to the target");
    }

    /// The rest-latch predicate: a stationary motion with absorbed residuals
    /// settles; any residual motion component, offset, or rotation keeps driving.
    #[test]
    fn physical_object_settled_requires_stationary_and_absorbed() {
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let stationary = MotionState::new(
            &Vector {
                x: 128.0,
                y: 128.0,
                z: 22.0,
            },
            &zero,
            &zero,
            &identity_rotation(),
            &zero,
            RegionHandle(0),
        );
        assert!(
            super::physical_object_settled(&stationary, Vec3::ZERO, Quat::IDENTITY),
            "zeros settle"
        );
        let moving = MotionState::new(
            &Vector {
                x: 128.0,
                y: 128.0,
                z: 22.0,
            },
            &Vector {
                x: 0.5,
                y: 0.0,
                z: 0.0,
            },
            &zero,
            &identity_rotation(),
            &zero,
            RegionHandle(0),
        );
        assert!(
            !super::physical_object_settled(&moving, Vec3::ZERO, Quat::IDENTITY),
            "a live velocity keeps driving"
        );
        assert!(
            !super::physical_object_settled(&stationary, Vec3::new(0.01, 0.0, 0.0), Quat::IDENTITY),
            "an unabsorbed positional residual keeps driving"
        );
        assert!(
            !super::physical_object_settled(
                &stationary,
                Vec3::ZERO,
                Quat::from_rotation_y(0.05_f32)
            ),
            "an unabsorbed rotation residual keeps driving"
        );
    }
}
