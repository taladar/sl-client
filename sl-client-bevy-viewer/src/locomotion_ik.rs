//! The locomotion pose adjusters (P31.14): the procedural corrections the reference
//! viewer layers over a *playing keyframe animation* so an avatar's legs agree with
//! how it is actually moving through the world.
//!
//! Four of the reference's motion classes, all of which need the three-joint IK
//! solver in [`crate::ik`] or the ground under the avatar's feet:
//!
//! - **`LLWalkAdjustMotion`** (`llkeyframewalkmotion.cpp`) — the always-on servo
//!   that runs while any walk animation is signalled. It measures how far the
//!   *planted* foot slides backwards against the ground each frame and publishes a
//!   `"Walk Speed"` multiplier that makes the animation's foot speed match the
//!   avatar's ground speed. This is what kills foot-skate: without it, an avatar
//!   moved by the simulator at a speed the walk cycle was not authored for either
//!   moonwalks or scrabbles.
//! - **`LLKeyframeWalkMotion`** (same file) — the walks / runs / turns themselves,
//!   whose only job beyond a plain keyframe motion is to advance their playback clock
//!   by `dt * "Walk Speed"` instead of by wall time. That happens in
//!   [`crate::animations`], which owns the clock; this module only computes the speed.
//! - **`LLKeyframeStandMotion`** (`llkeyframestandmotion.cpp`) — the stands and the
//!   crouch, whose lower body is **foot-IK'd**: each ankle is projected down onto the
//!   ground, the hip / knee are solved to put it there, and the ankle is rolled to lie
//!   flat on the slope. On sloped terrain this is the difference between feet planted
//!   on the hill and feet buried in it or floating over it. (The "a standing avatar's
//!   legs follow the camera" effect is this same solve seen from the other side: the
//!   pelvis turns with the avatar while the ankles stay planted, so the legs twist to
//!   keep up.)
//! - **`LLKeyframeFallMotion`** (`llkeyframefallmotion.cpp`) — `standup`, the landing
//!   recovery, which starts with the pelvis aligned to the **ground normal** (an
//!   avatar that lands on a slope lies along it) and blends back upright over the
//!   second half of the motion.
//! - **`LLFlyAdjustMotion`** (`llkeyframewalkmotion.cpp`) — the always-on bank while
//!   airborne: the pelvis rolls into a turn proportionally to the avatar's angular
//!   velocity, scaled in above 7 m/s.
//!
//! **Faithfulness notes.** Two things in the reference are vestigial and are ported
//! as they actually behave, not as they read:
//!
//! - `LLWalkAdjustMotion`'s `mPelvisOffset` — the "pelvis lag" that would slide the
//!   hips back to absorb foot drift — is **commented out** upstream, with a `FIXME`
//!   saying it fights the speed adjustment (it delays the servo enough to sometimes
//!   play the walk backwards). The motion still writes the (always-zero) offset to the
//!   pelvis every frame purely to stay "active". Porting the live behaviour means
//!   porting the speed servo and *not* the pelvis offset; a viewer that added it back
//!   would look worse, not more correct.
//! - `LLKeyframeStandMotion`'s `mTrackAnkles` is set to `true` in three places and to
//!   `false` in none, so the ankle-*locking* it once gated (freeze the foot targets
//!   while the pelvis turns) is dead: the modern reference re-projects the ankles onto
//!   the ground every frame. This ports that.
//!
//! **Where the ground comes from.** [`crate::ground`] probes it the way the reference's
//! `getGround` does — a short vertical raycast against the rendered world — so the feet
//! plant on a prim ramp, a staircase or a skybox platform exactly as readily as on the
//! terrain. (A terrain lookup alone could not; nor could a physics query, since a static
//! walkable prim carries no avian collider.)
//!
//! **The one deliberate deviation.** The foot IK aims each ankle at its keyframe height
//! **displaced by how much higher or lower the ground is under that foot than under the
//! avatar's root**, where the reference aims it at the ground's *absolute* height. This
//! is the same correction expressed in a different frame, not a weaker one: the
//! reference's skeleton root sits at the **pelvis** (`root_pos.z = ground +
//! mPelvisToFoot`), whereas this viewer's body root hangs the whole skeleton — pelvis at
//! its local rest offset — below the reported capsule-centre Z by the resolved root drop
//! (`avatars::root_drop_from_metrics`, R23), so the root is *near* the sole but not the
//! reference's frame, and aiming our ankle at the absolute ground would drive it too low
//! and bury the foot. As a bonus, a difference of two ground samples is insensitive to
//! the absolute root placement — it survived the R23 re-plant unchanged — and cancels
//! out any error in the simulator-reported avatar position, instead of absorbing it into
//! the legs as a permanent crouch. See [`plant_foot`].
//!
//! # A standing leg is a near-singular IK chain
//!
//! Everything delicate here follows from one fact, and it is worth stating plainly because
//! it produced two separate live bugs that no unit test would have caught:
//!
//! **A standing leg sits at ~99.5% of full extension, and the IK's gain at full extension
//! is unbounded.** The ankle-to-hip distance is *stationary* in the knee angle when the leg
//! is straight, so near there an arbitrarily small change in the goal becomes an
//! arbitrarily large change in the knee. Measured on a 20° ramp: 2 cm of ankle movement
//! swung the knee by 50°.
//!
//! That amplifier turned two innocuous-looking choices into visible faults:
//!
//! - **Probing the ground under the *posed* ankle closed a feedback loop.** A goal out of
//!   the leg's reach makes the solver straighten the leg and *point* it at the goal, so the
//!   ankle lands short — and sideways. The next probe then sampled the ground somewhere
//!   else, which moved the goal, which moved the ankle: a limit cycle that buzzed the knees
//!   on every slope, and which also *suppressed the correct answer* (the displacement sat
//!   near zero instead of the true ±2 cm). Fixed by casting through the **pre-IK** ankle
//!   ([`crate::ground::AvatarGround::targets`]) and by clamping the goal into reach
//!   ([`clamp_goal_to_reach`]), so the solve always lands where the probe expects.
//! - **Clamping the goal to a flat 99.5% of reach lifted the avatar.** A standing leg is
//!   already past that, so the clamp fired even with *nothing to correct*, tugging both
//!   ankles up toward the hips and rising onto the toes on every stop. The limit must never
//!   be tighter than the pose already is — see [`clamp_goal_to_reach`] again.
//!
//! Neither is in the reference, and neither is a bug *in* it: the reference ground-plants
//! its own root and probes a stable frame, so the loop never closes for it. Ours is
//! dead-reckoned from the simulator, so it does. The invariant to preserve if this code is
//! ever touched: **on flat ground the foot IK must be a bit-for-bit no-op**, and the ground
//! probe must never depend on anything the IK produced.

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::{AgentKey, AnimationPose, RegionHandle};

use crate::coords::{bevy_to_sl_vec, metres_to_f32, sl_to_bevy_rotation};
use crate::ground::{AgentGround, GroundHit};
use crate::ik::{Chain, JointSolver};
use crate::look_at::smooth_interpolant;
use crate::physics::AvatarMotion;

/// Maximum ground speed (m/s) the walk servo adjusts for; beyond it the avatar is
/// moving too fast for the walk cycle to keep up at all
/// (`MAX_WALK_PLAYBACK_SPEED`).
const MAX_WALK_PLAYBACK_SPEED: f32 = 8.0;

/// Ground speed (m/s) below which the avatar counts as standing / turning in place
/// rather than walking, so the servo relaxes instead of measuring foot slip
/// (`MIN_WALK_SPEED`).
const MIN_WALK_SPEED: f32 = 0.1;

/// Smallest frame time the servo will divide by (`TIME_EPSILON`).
const TIME_EPSILON: f32 = 0.001;

/// Largest frame time the servo will integrate over — a longer stall is clamped so a
/// hitch does not produce a wild speed correction (`MAX_TIME_DELTA`).
const MAX_TIME_DELTA: f32 = 2.0;

/// How fast the published walk speed may change, per second (`SPEED_ADJUST_MAX_SEC`).
const SPEED_ADJUST_MAX_SEC: f32 = 2.0;

/// Absolute ceiling on the walk-animation playback multiplier (`ANIM_SPEED_MAX`).
const ANIM_SPEED_MAX: f32 = 1.5;

/// Half-life (seconds) of the walk-speed servo's smoothing
/// (`SPEED_ADJUST_TIME_CONSTANT`).
const SPEED_ADJUST_TIME_CONSTANT: f32 = 0.1;

/// Half-life (seconds) the standing branch relaxes the published walk speed back to
/// 1 with (the reference's literal `0.2f` in `LLWalkAdjustMotion::onUpdate`).
const SPEED_RELAX_TIME_CONSTANT: f32 = 0.2;

/// Maximum fly bank, radians (`MAX_ROLL`).
const MAX_ROLL: f32 = 0.6;

/// Half-life (seconds) of the fly bank's smoothing (the reference's
/// `F32Milliseconds(100.f)`).
const FLY_ROLL_TIME_CONSTANT: f32 = 0.1;

/// The angular-velocity range (rad/s about the up axis) the fly bank reads, clamped
/// (`llclamp(ang_vel.mV[VZ], -4.f, 4.f)`).
const FLY_ROLL_YAW_RATE_LIMIT: f32 = 4.0;

/// The ground-speed range (m/s) the fly bank scales in over: no bank below the first,
/// full [`MAX_ROLL`] at the second (`clamp_rescale(speed, 7.f, 15.f, …)`).
const FLY_ROLL_SPEED_RANGE: (f32, f32) = (7.0, 15.0);

/// The pole vector of both leg IK chains, in the pelvis's frame: Second Life `+X` is
/// forward, so the knees bend forwards (`LLKeyframeStandMotion::onActivate`).
const LEG_POLE_VECTOR: Vec3 = Vec3::X;

/// The left knee's hinge axis in its own frame — very nearly `+Y` (the avatar's left),
/// with the small `X` bias the reference gives it so the knee tracks slightly outward
/// (`mIKLeft.setBAxis(LLVector3(0.05f, 1.0f, 0.0f))`).
const LEFT_KNEE_AXIS: Vec3 = Vec3::new(0.05, 1.0, 0.0);

/// The right knee's hinge axis, mirrored (`LLVector3(-0.05f, 1.0f, 0.0f)`).
const RIGHT_KNEE_AXIS: Vec3 = Vec3::new(-0.05, 1.0, 0.0);

/// Half-life (seconds) over which the foot IK eases in and out as the avatar leaves and
/// regains the ground, so stepping off a ledge fades the correction rather than snapping
/// the legs straight.
const FOOT_IK_WEIGHT_TIME_CONSTANT: f32 = 0.15;

/// How much of the leg's length the foot IK keeps in reserve, so the goal can never drive
/// it fully straight. A standing leg already sits at ~99.5% of full extension, and the
/// IK's gain *at* full extension is unbounded — the ankle-to-hip distance is stationary in
/// the knee angle there, so an arbitrarily small change in the ground turns into an
/// arbitrarily large change in the knee. This backs the goal off that singularity without
/// visibly bending a standing leg.
const LEG_REACH_MARGIN: f32 = 0.005;

/// Half-life (seconds) of the smoothing on each foot's ground displacement. The ground
/// under a foot is not perfectly steady — the avatar's reported position is dead-reckoned
/// between object updates, and a moving foot crosses between terrain triangles — and a
/// near-straight leg amplifies whatever noise reaches it. Filtering the *input* is the
/// cheapest place to stop that; foot planting is a slow, static effect, so the lag costs
/// nothing visible.
const FOOT_GROUND_TIME_CONSTANT: f32 = 0.08;

/// A linear rescale of `x` from `x1..x2` onto `y1..y2`, clamped to the `y` range in
/// whichever order it was given — the reference's `clamp_rescale` (`llmath.h`), used
/// by the walk servo's floor and the fly bank's speed ramp.
pub(crate) fn clamp_rescale(x: f32, x1: f32, x2: f32, y1: f32, y2: f32) -> f32 {
    let span = x2 - x1;
    if span.abs() < f32::EPSILON {
        return y1;
    }
    let rescaled = y1 + (x - x1) * (y2 - y1) / span;
    rescaled.clamp(y1.min(y2), y1.max(y2))
}

/// Component-wise vector subtraction (`a - b`), avoiding the glam `-` operator the
/// workspace `arithmetic_side_effects` lint trips on.
fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise vector addition (`a + b`).
fn vadd(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// Component-wise vector scaling (`v * s`).
fn vscale(v: Vec3, s: f32) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

/// The rotation whose local `+X` / `+Y` / `+Z` are the given orthonormal `forward` /
/// `left` / `up` — the reference's `LLQuaternion(fwd, left, up)` basis constructor.
fn basis(forward: Vec3, left: Vec3, up: Vec3) -> Quat {
    Quat::from_mat3(&Mat3::from_cols(forward, left, up))
}

/// A rotation aligning `+Z` to `up` while keeping `+X` as close to `forward` as the
/// orthogonality allows — the reference's `up % dir; dir = left % up` re-orthogonalise
/// followed by the basis constructor. A degenerate pair yields the identity.
fn basis_from_up(forward: Vec3, up: Vec3) -> Quat {
    let up = up.normalize_or_zero();
    let left = up.cross(forward.normalize_or_zero()).normalize_or_zero();
    if left.length_squared() < f32::EPSILON {
        return Quat::IDENTITY;
    }
    basis(left.cross(up), left, up)
}

/// The walk servo's steady-state playback multiplier: the factor that would make the
/// animation's foot speed equal the avatar's ground `speed`, given the planted foot's
/// measured `foot_speed`. A foot that slides forward with the avatar has (near) zero
/// ground speed — the cycle is far too slow, so the multiplier saturates at
/// [`ANIM_SPEED_MAX`]. The floor rises with speed so a barely-moving avatar does not
/// stall the cycle entirely (the reference's `min_speed_multiplier`).
fn walk_speed_multiplier(speed: f32, foot_speed: f32) -> f32 {
    let floor = clamp_rescale(speed, 0.0, 1.0, 0.0, 0.1);
    if foot_speed <= f32::EPSILON {
        return ANIM_SPEED_MAX;
    }
    (speed / foot_speed).clamp(floor, ANIM_SPEED_MAX)
}

/// The fly bank's steady-state target roll (radians) for a yaw rate (rad/s about the
/// avatar's up axis) and ground `speed` — `LLFlyAdjustMotion::onUpdate`'s
/// `llclamp(ang_vel.mV[VZ], -4, 4) * clamp_rescale(speed, 7, 15, 0, -MAX_ROLL)`. No
/// bank below [`FLY_ROLL_SPEED_RANGE`]`.0`; full authority above its `.1`.
fn fly_roll_target(yaw_rate: f32, speed: f32) -> f32 {
    let roll_factor = clamp_rescale(
        speed,
        FLY_ROLL_SPEED_RANGE.0,
        FLY_ROLL_SPEED_RANGE.1,
        0.0,
        -MAX_ROLL,
    );
    yaw_rate.clamp(-FLY_ROLL_YAW_RATE_LIMIT, FLY_ROLL_YAW_RATE_LIMIT) * roll_factor
}

/// The skeleton indices of the joints the locomotion adjusters drive, resolved once
/// per avatar per frame. A joint the skeleton lacks is [`None`]; the leg IK needs all
/// six leg joints plus the pelvis and simply does not run without them.
#[derive(Clone, Copy, Default)]
pub(crate) struct LegJoints {
    /// `mPelvis` — the fly bank's and fall recovery's joint, and the parent frame the
    /// leg IK's pole vector is expressed in.
    pub(crate) pelvis: Option<usize>,
    /// `mHipLeft` / `mKneeLeft` / `mAnkleLeft`.
    pub(crate) left: Option<(usize, usize, usize)>,
    /// `mHipRight` / `mKneeRight` / `mAnkleRight`.
    pub(crate) right: Option<(usize, usize, usize)>,
}

impl LegJoints {
    /// Resolve the joint indices from a skeleton's name lookup.
    pub(crate) fn resolve(joint_index: impl Fn(&str) -> Option<usize>) -> Self {
        let leg = |hip: &str, knee: &str, ankle: &str| {
            Some((joint_index(hip)?, joint_index(knee)?, joint_index(ankle)?))
        };
        Self {
            pelvis: joint_index("mPelvis"),
            left: leg("mHipLeft", "mKneeLeft", "mAnkleLeft"),
            right: leg("mHipRight", "mKneeRight", "mAnkleRight"),
        }
    }
}

/// Which locomotion adjusters an avatar's currently-playing animation set calls for,
/// derived from the signalled animations by [`crate::animations`] (which owns the
/// playback clock the fall recovery's progress comes from).
#[derive(Clone, Copy, Default)]
pub(crate) struct AdjusterAnims {
    /// Any of the reference's `AGENT_WALK_ANIMS` is playing, so `LLWalkAdjustMotion`
    /// is active and publishes a walk speed.
    pub(crate) walking: bool,
    /// An `LLKeyframeStandMotion` (a stand, or the crouch) is playing, so its lower
    /// body is foot-IK'd.
    pub(crate) standing: bool,
    /// The `LLKeyframeFallMotion` (`standup`) is playing: its elapsed time and its
    /// duration, from which the recovery blends the pelvis back upright.
    pub(crate) fall: Option<(f32, f32)>,
}

/// One avatar's cross-frame adjuster state: the walk servo's accumulators and foot
/// history, the fly bank's damped roll, the eased foot-IK weight, and the fall
/// recovery's ground alignment (captured once, when the motion starts).
#[derive(Default)]
struct AgentAdjust {
    /// The previous frame's left-ankle position in **global** metres, projected onto
    /// the horizontal plane. Global rather than region-local so a region crossing does
    /// not read as a 256 m foot slide (the reference uses `getPosGlobalFromAgent` for
    /// exactly this). `None` until the first frame the servo runs.
    last_left_foot: Option<Vec2>,
    /// The previous frame's right-ankle position, likewise.
    last_right_foot: Option<Vec2>,
    /// The servo's smoothed speed multiplier before the direction factor
    /// (`mAdjustedSpeed`).
    adjusted_speed: f32,
    /// The published walk-animation playback multiplier (`mAnimSpeed`): the smoothed
    /// multiplier signed by how much of the avatar's motion is *forward*, so walking
    /// backwards plays the cycle backwards and strafing slows it.
    anim_speed: f32,
    /// The fly bank's critically-damped roll about the pelvis's up axis (`mRoll`).
    roll: f32,
    /// The eased `[0, 1]` weight of the foot IK, so it fades in and out with the ground
    /// instead of popping.
    foot_ik_weight: f32,
    /// Each foot's smoothed ground displacement (left, right), metres — the filtered
    /// input the leg IK is actually solved against. See [`FOOT_GROUND_TIME_CONSTANT`].
    foot_displacement: (f32, f32),
    /// The fall recovery's pelvis rotation onto the ground normal
    /// (`mRotationToGroundNormal`), captured when the `standup` motion starts (the
    /// reference captures it in `onActivate`) and held for the motion's life.
    fall_ground_rotation: Option<Quat>,
}

/// Per-avatar [`AgentAdjust`] state, keyed by agent, retained across frames so the
/// walk servo, fly bank and IK weight are continuous.
#[derive(Resource, Default)]
pub(crate) struct LocomotionAdjust {
    /// The adjuster state of each rigged avatar seen so far.
    states: HashMap<AgentKey, AgentAdjust>,
}

impl LocomotionAdjust {
    /// The walk-animation playback multiplier published for `agent` — the reference's
    /// `"Walk Speed"` animation data, read by [`crate::animations`] to scale the
    /// playback clock of the avatar's [`Walk`](sl_anim::KeyframeMotionClass::Walk)
    /// motions. `1.0` (unscaled) for an avatar with no walk animation active, which is
    /// exactly what the reference's *absent* `"Walk Speed"` data means.
    #[must_use]
    pub(crate) fn walk_speed(&self, agent: AgentKey) -> f32 {
        self.states
            .get(&agent)
            .map_or(1.0, |state| state.anim_speed)
    }

    /// Forget every avatar not in `live` (they have despawned), so the state map does
    /// not grow without bound over a long session.
    pub(crate) fn retain(&mut self, live: &impl Fn(AgentKey) -> bool) {
        self.states.retain(|&agent, _state| live(agent));
    }
}

/// The avatar-local Second Life skeleton frame ↔ the region frame: the deformed
/// skeleton's world matrices are expressed in the former (origin at the avatar's body
/// root, axes the avatar's own), while the terrain and the foot history are in the
/// latter.
#[derive(Clone, Copy)]
struct AvatarFrame {
    /// The skeleton frame's origin in region coordinates: the avatar's reported
    /// horizontal position, at the body root's height (the root is planted a pelvis
    /// height below the reported position, i.e. at the feet).
    origin: Vec3,
    /// The avatar's orientation: maps skeleton-frame vectors into the region frame.
    rotation: Quat,
    /// The region the origin is expressed in.
    region: RegionHandle,
}

impl AvatarFrame {
    /// The frame an avatar's rendered body-root global places its skeleton in: the Bevy
    /// global carries the Second Life → Bevy basis change on top of the avatar's own
    /// orientation and world position, so undoing that basis change recovers both in the
    /// region's own Second Life frame.
    fn from_root(root: &GlobalTransform, region: RegionHandle) -> Self {
        let origin = bevy_to_sl_vec(root.translation());
        Self {
            origin: Vec3::new(origin.x, origin.y, origin.z),
            rotation: sl_to_bevy_rotation().inverse().mul_quat(root.rotation()),
            region,
        }
    }

    /// A skeleton-frame point in region coordinates.
    fn to_region(self, local: Vec3) -> Vec3 {
        vadd(self.origin, self.rotation.mul_vec3(local))
    }

    /// A region-frame direction in skeleton coordinates.
    fn dir_to_local(self, region: Vec3) -> Vec3 {
        self.rotation.inverse().mul_vec3(region)
    }

    /// A region-frame horizontal point in **global** metres — the frame the foot
    /// history is kept in, so a region crossing does not look like a foot slide.
    fn to_global_xy(self, region: Vec3) -> Vec2 {
        let (gx, gy) = self.region.global_coordinates();
        Vec2::new(metres_to_f32(gx) + region.x, metres_to_f32(gy) + region.y)
    }
}

/// Everything one avatar's adjusters need from outside this module for one frame.
pub(crate) struct AdjustInput<'a> {
    /// The avatar being adjusted.
    pub(crate) agent: AgentKey,
    /// Every joint's deformed **world** matrix in the avatar-local Second Life frame,
    /// with the keyframe + idle + look-at pose already folded in — the pose the
    /// adjusters correct, and where the IK reads the current leg geometry from.
    pub(crate) world: &'a [Mat4],
    /// The avatar body root's Bevy global — the skeleton frame's placement in the world.
    ///
    /// Deliberately the **rendered** pose (P31.2's dead-reckoned interpolation), not the
    /// last reported one on [`motion`](Self::motion): the walk servo differences its feet's
    /// world positions frame to frame, and the reported position only moves when an
    /// `ObjectUpdate` arrives. Anchoring the feet to it would leave them standing still
    /// between updates while the walk cycle swung them, which the servo would read as the
    /// planted foot sliding *backwards* — and it would obediently slow the walk down.
    pub(crate) root: &'a GlobalTransform,
    /// The skeleton indices of the joints the adjusters drive.
    pub(crate) joints: LegJoints,
    /// The avatar's authoritative motion, absent for a coarse (minimap-only) avatar.
    pub(crate) motion: Option<&'a AvatarMotion>,
    /// What is under the avatar's root and each of its feet this frame — terrain, prim
    /// ramp, platform or nothing at all (see [`crate::ground`]).
    pub(crate) ground: AgentGround,
    /// Which adjusters the playing animation set calls for.
    pub(crate) anims: AdjusterAnims,
    /// The frame time, seconds.
    pub(crate) dt: f32,
}

/// A **Bevy world** point in the avatar-local Second Life skeleton frame (the frame the
/// deformed world matrices and the [`AnimationPose`] live in).
fn to_local_point(root: &GlobalTransform, world: Vec3) -> Vec3 {
    root.affine().inverse().transform_point3(world)
}

/// A **Bevy world** direction in the avatar-local Second Life skeleton frame.
fn to_local_dir(root: &GlobalTransform, world: Vec3) -> Vec3 {
    root.rotation().inverse().mul_vec3(world)
}

/// The knee's bend angle in degrees — the angle between the thigh (`hip → knee`) and the
/// shin (`knee → ankle`) — read out of a set of deformed world matrices. `0` is a
/// perfectly straight leg.
///
/// A diagnostic, but a load-bearing one: it is the number that distinguishes an unstable
/// *ground* (the displacement moving under the feet) from an unstable *solve* (the knee
/// flipping between the two mirror solutions that both reach the goal), which look
/// identical from outside.
#[must_use]
pub(crate) fn knee_bend_degrees(world: &[Mat4], hip: usize, knee: usize, ankle: usize) -> f32 {
    let (Some((hip_pos, _)), Some((knee_pos, _)), Some((ankle_pos, _))) = (
        joint_pose(world, hip),
        joint_pose(world, knee),
        joint_pose(world, ankle),
    ) else {
        return 0.0;
    };
    let thigh = vsub(knee_pos, hip_pos);
    let shin = vsub(ankle_pos, knee_pos);
    thigh.angle_between(shin).to_degrees()
}

/// The world position and rotation of joint `index` in the avatar-local Second Life
/// frame, read out of the deformed world matrices.
fn joint_pose(world: &[Mat4], index: usize) -> Option<(Vec3, Quat)> {
    let matrix = world.get(index)?;
    let (_scale, rotation, translation) = matrix.to_scale_rotation_translation();
    Some((translation, rotation))
}

/// Fold `delta` onto joint `index`'s current pose rotation (`base · delta`) — the
/// additive-blend convention the reference's `LLJointStateBlender` applies to the
/// always-on adjusters, which compose in the joint's own frame *before* its blended
/// keyframe rotation.
fn compose(pose: &mut AnimationPose, index: Option<usize>, delta: Quat) {
    if let Some(index) = index {
        let base = pose.rotation(index).unwrap_or(Quat::IDENTITY);
        pose.set_rotation(index, base.mul_quat(delta));
    }
}

/// What the adjusters did to one avatar this frame, for the live diagnostic
/// (`SL_VIEWER_LOG_LOCOMOTION_IK=1`). The authoritative walk speed lives on
/// [`LocomotionAdjust`], which [`crate::animations`] reads next frame to scale the walk
/// motions' playback clock; this is purely a report.
#[derive(Clone, Copy, Default)]
pub(crate) struct AdjustReport {
    /// The walk-animation playback multiplier published for the avatar.
    pub(crate) walk_speed: f32,
    /// The eased `[0, 1]` weight the foot IK is currently applied at.
    pub(crate) foot_ik_weight: f32,
    /// The fly bank, radians.
    pub(crate) roll: f32,
    /// Whether the ground probe found a surface under the body root, and under each
    /// ankle — the direct evidence that the raycast reaches a prim ramp, not just the
    /// terrain.
    pub(crate) ground: (bool, bool, bool),
    /// How far each foot's ground sits above (+) or below (−) the ground under the
    /// avatar's root, metres — the displacement the foot IK actually applies. Non-zero
    /// exactly when the avatar is standing on something sloped.
    pub(crate) displacement: (f32, f32),
    /// The tilt of the ground under the root away from vertical, degrees.
    pub(crate) slope_deg: f32,
}

/// Run every locomotion adjuster the playing animation set calls for, folding the
/// corrections into `pose` in place, and report what it did.
pub(crate) fn apply(
    pose: &mut AnimationPose,
    adjust: &mut LocomotionAdjust,
    input: &AdjustInput<'_>,
) -> AdjustReport {
    let state = adjust.states.entry(input.agent).or_default();
    let dt = input.dt.clamp(TIME_EPSILON, MAX_TIME_DELTA);
    let Some(motion) = input.motion else {
        // A coarse avatar has no velocity or ground: nothing to adjust.
        state.anim_speed = 1.0;
        return AdjustReport {
            walk_speed: 1.0,
            ..AdjustReport::default()
        };
    };
    let frame = AvatarFrame::from_root(input.root, motion.region());

    // --- LLWalkAdjustMotion: match the walk cycle's foot speed to the ground ---
    if input.anims.walking {
        walk_adjust(state, input, motion, &frame, dt);
    } else {
        // The reference stops the motion and removes the `"Walk Speed"` data, which
        // makes every walk motion play at its authored speed again; the equivalent
        // here is publishing 1 and forgetting the foot history.
        state.anim_speed = 1.0;
        state.adjusted_speed = 0.0;
        state.last_left_foot = None;
        state.last_right_foot = None;
    }

    // --- LLFlyAdjustMotion: bank into a turn while airborne ---
    // The reference gates this on `mInAir && !isSitting()`. An avatar with no ground
    // within the probe's reach is exactly that — and unlike the reference's `mInAir`
    // (which only its *own* avatar tracks) the probe answers it for every avatar.
    let airborne = !input.anims.walking && input.ground.root.is_none();
    if airborne {
        fly_adjust(state, motion, dt);
    } else {
        state.roll = 0.0;
    }
    if state.roll.abs() > f32::EPSILON {
        compose(
            pose,
            input.joints.pelvis,
            Quat::from_axis_angle(Vec3::Z, state.roll),
        );
    }

    // --- LLKeyframeFallMotion: land along the slope, then rise upright ---
    match (input.anims.fall, input.ground.root) {
        (Some((elapsed, duration)), Some(ground)) => {
            fall_adjust(pose, state, input, &ground, elapsed, duration);
        }
        _not_recovering => state.fall_ground_rotation = None,
    }

    // --- LLKeyframeStandMotion: plant the ankles on the ground ---
    // Eased rather than switched: the weight decides how much of the solved leg pose
    // replaces the keyframe one, so a foot leaving the ground never pops.
    let target_weight = if input.anims.standing && input.ground.root.is_some() {
        1.0
    } else {
        0.0
    };
    let ease = smooth_interpolant(FOOT_IK_WEIGHT_TIME_CONSTANT, dt);
    state.foot_ik_weight += (target_weight - state.foot_ik_weight) * ease;
    let mut displacement = (0.0, 0.0);
    if let Some(root_ground) = input.ground.root
        && state.foot_ik_weight > f32::EPSILON
    {
        // Smooth each foot's raw ground displacement before it reaches the solve, so the
        // near-straight leg's huge IK gain has nothing high-frequency to amplify.
        let ground_ease = smooth_interpolant(FOOT_GROUND_TIME_CONSTANT, dt);
        let raw = |hit: Option<GroundHit>| hit.map_or(0.0, |hit| hit.point.y - root_ground.point.y);
        state.foot_displacement.0 +=
            (raw(input.ground.left) - state.foot_displacement.0) * ground_ease;
        state.foot_displacement.1 +=
            (raw(input.ground.right) - state.foot_displacement.1) * ground_ease;
        let weight = state.foot_ik_weight;
        let (left, right) = state.foot_displacement;
        displacement = (
            plant_foot(pose, input, Leg::Left, weight, left),
            plant_foot(pose, input, Leg::Right, weight, right),
        );
    }

    AdjustReport {
        walk_speed: state.anim_speed,
        foot_ik_weight: state.foot_ik_weight,
        roll: state.roll,
        ground: (
            input.ground.root.is_some(),
            input.ground.left.is_some(),
            input.ground.right.is_some(),
        ),
        displacement,
        slope_deg: input.ground.root.map_or(0.0, |ground| {
            to_local_dir(input.root, ground.normal)
                .angle_between(Vec3::Z)
                .to_degrees()
        }),
    }
}

/// `LLWalkAdjustMotion::onUpdate`: measure how far the planted foot slid against the
/// ground this frame and servo the walk animation's playback speed until it stops
/// sliding.
fn walk_adjust(
    state: &mut AgentAdjust,
    input: &AdjustInput<'_>,
    motion: &AvatarMotion,
    frame: &AvatarFrame,
    dt: f32,
) {
    // The avatar's horizontal velocity, in the region frame.
    let velocity = motion.sl_velocity();
    let velocity = Vec3::new(velocity.x, velocity.y, 0.0);
    let speed = velocity.length().clamp(0.0, MAX_WALK_PLAYBACK_SPEED);

    // Both ankles' horizontal positions in global metres, and how far each moved since
    // the previous frame. The *planted* foot is the one being pushed backwards hardest
    // against the direction of travel, so it is the one whose slip measures the error.
    let ankle_global = |leg: Option<(usize, usize, usize)>| -> Option<Vec2> {
        let (_hip, _knee, ankle) = leg?;
        let (position, _rotation) = joint_pose(input.world, ankle)?;
        Some(frame.to_global_xy(frame.to_region(position)))
    };
    let left = ankle_global(input.joints.left);
    let right = ankle_global(input.joints.right);

    if speed <= MIN_WALK_SPEED {
        // Standing or turning in place: relax the published speed back to 1 rather
        // than servoing on a foot slip that has no direction of travel to measure
        // against.
        let relax = smooth_interpolant(SPEED_RELAX_TIME_CONSTANT, dt);
        state.anim_speed += (1.0 - state.anim_speed) * relax;
        state.last_left_foot = left;
        state.last_right_foot = right;
        return;
    }

    let delta = |now: Option<Vec2>, last: Option<Vec2>| -> Option<Vec3> {
        let (now, last) = (now?, last?);
        Some(Vec3::new(now.x - last.x, now.y - last.y, 0.0))
    };
    let left_delta = delta(left, state.last_left_foot);
    let right_delta = delta(right, state.last_right_foot);
    state.last_left_foot = left;
    state.last_right_foot = right;

    // Whichever foot is moving *least* along the direction of travel (most negative
    // when projected on the velocity) is the planted one.
    let slip = match (left_delta, right_delta) {
        (Some(left), Some(right)) => {
            if right.dot(velocity) < left.dot(velocity) {
                right
            } else {
                left
            }
        }
        (Some(only), None) | (None, Some(only)) => only,
        // The first frame of a walk has no previous foot position to difference
        // against; hold the current speed until the next one.
        (None, None) => return,
    };

    let direction = velocity.normalize_or_zero();
    // The planted foot's true ground speed: how fast the avatar moves, less how fast
    // the foot is sliding with it. A foot that slides forward as fast as the avatar
    // moves has zero ground speed — the animation is playing far too slowly.
    let foot_speed = (speed - slip.dot(direction) / dt).max(0.0);

    let desired = walk_speed_multiplier(speed, foot_speed);

    // Smooth toward it, then rate-limit how fast the smoothed value may move, so a
    // single bad frame cannot jerk the playback speed.
    let smoothed = state.adjusted_speed
        + (desired - state.adjusted_speed) * smooth_interpolant(SPEED_ADJUST_TIME_CONSTANT, dt);
    let max_step = SPEED_ADJUST_MAX_SEC * dt;
    let step = (smoothed - state.adjusted_speed).clamp(-max_step, max_step);
    state.adjusted_speed += step;

    // Finally sign and scale by how *forward* the motion is, in the avatar's own frame:
    // walking backwards plays the cycle backwards, strafing slows it. Done last (as the
    // reference notes) so a direction change is responsive rather than filtered.
    let forward = frame.dir_to_local(direction).x;
    state.anim_speed = state.adjusted_speed * forward;
}

/// `LLFlyAdjustMotion::onUpdate`: roll the pelvis into a turn, proportionally to the
/// avatar's yaw rate and scaled in with speed.
fn fly_adjust(state: &mut AgentAdjust, motion: &AvatarMotion, dt: f32) {
    let target = fly_roll_target(
        motion.sl_angular_velocity().z,
        motion.sl_velocity().length(),
    );
    state.roll += (target - state.roll) * smooth_interpolant(FLY_ROLL_TIME_CONSTANT, dt);
}

/// `LLKeyframeFallMotion`: the pelvis starts lying along the ground it landed on and
/// blends back upright over the recovery's second half.
fn fall_adjust(
    pose: &mut AnimationPose,
    state: &mut AgentAdjust,
    input: &AdjustInput<'_>,
    ground: &GroundHit,
    elapsed: f32,
    duration: f32,
) {
    // Captured once, when the motion starts — the reference does it in `onActivate`,
    // so an avatar that lands on a slope keeps lying along *that* slope even as the
    // recovery carries it elsewhere.
    let ground_rotation = *state.fall_ground_rotation.get_or_insert_with(|| {
        // The ground normal in the avatar's own frame, and the forward axis
        // re-orthogonalised against it.
        let normal = to_local_dir(input.root, ground.normal).normalize_or(Vec3::Z);
        let forward = vsub(Vec3::X, vscale(normal, Vec3::X.dot(normal))).normalize_or_zero();
        if forward.length_squared() < f32::EPSILON {
            return Quat::IDENTITY;
        }
        basis(forward, normal.cross(forward), normal)
    });
    if duration <= 0.0 {
        return;
    }
    // 0 for the first half of the motion (fully aligned to the ground), ramping to 1
    // by three-quarters through (fully upright).
    let upright = clamp_rescale(elapsed / duration, 0.5, 0.75, 0.0, 1.0);
    let applied = ground_rotation.slerp(Quat::IDENTITY, upright);
    if let Some(pelvis) = input.joints.pelvis {
        let keyframe = pose.rotation(pelvis).unwrap_or(Quat::IDENTITY);
        // The reference's `keyframe * rotation_to_ground` — a post-multiply in Linden
        // order, i.e. applied in the pelvis's *parent* (avatar) frame, which is the
        // frame the ground rotation was computed in.
        pose.set_rotation(pelvis, applied.mul_quat(keyframe));
    }
}

/// Pull a foot-IK `goal` back inside the leg's reach, so the solve can always actually
/// land the ankle on it.
///
/// Two distinct jobs, both learned the hard way on a live grid:
///
/// - **The goal must be reachable.** A goal the leg cannot stretch to makes the solver do
///   what the reference does — straighten the leg and *point* it at the goal — which lands
///   the ankle short of the goal and, crucially, at a different horizontal position. The
///   ground is sampled under the ankle, so that moved the probe, which moved the goal,
///   which moved the ankle: a limit cycle that set the knees buzzing on every slope.
/// - **…but never tighter than the pose already is.** The margin backs the limit off full
///   extension, where the IK's gain is unbounded (ankle-to-hip distance is stationary in
///   the knee angle there, so a millimetre of ground noise becomes degrees of knee). A
///   standing leg, though, already sits at ~99.5% of full extension — so a flat ceiling at
///   99.5% would clamp goals that need no correction at all, tugging both ankles up toward
///   the hips and lifting the avatar onto its toes every time it stopped walking. Taking
///   the *looser* of the two limits keeps an uncorrected foot (zero displacement, flat
///   ground) exactly where the animation put it, while still holding back a foot that
///   reaches beyond the animation's own extension.
fn clamp_goal_to_reach(hip: Vec3, knee: Vec3, ankle: Vec3, goal: Vec3) -> Vec3 {
    let full_reach = vsub(knee, hip).length() + vsub(ankle, knee).length();
    let posed_reach = vsub(ankle, hip).length();
    let reach = (full_reach * (1.0 - LEG_REACH_MARGIN)).max(posed_reach);
    let to_goal = vsub(goal, hip);
    if to_goal.length() <= reach {
        return goal;
    }
    vadd(hip, vscale(to_goal.normalize_or_zero(), reach))
}

/// Which leg a foot-plant solve is for.
#[derive(Clone, Copy)]
enum Leg {
    /// The left leg (`mHipLeft` / `mKneeLeft` / `mAnkleLeft`).
    Left,
    /// The right leg.
    Right,
}

/// `LLKeyframeStandMotion`'s foot IK for one leg: raise or lower the ankle onto the ground
/// under it, solve the hip and knee to put it there, and roll the ankle flat onto the
/// surface. `weight` blends the whole correction against the animation's own leg pose;
/// `displacement` is the already-smoothed height of this foot's ground above the root's.
///
/// Returns the displacement it actually aimed by, for the diagnostic.
fn plant_foot(
    pose: &mut AnimationPose,
    input: &AdjustInput<'_>,
    leg: Leg,
    weight: f32,
    displacement: f32,
) -> f32 {
    let (chain_joints, knee_axis, foot_ground) = match leg {
        Leg::Left => (input.joints.left, LEFT_KNEE_AXIS, input.ground.left),
        Leg::Right => (input.joints.right, RIGHT_KNEE_AXIS, input.ground.right),
    };
    // No ground under this foot (it is out over a ledge): leave the leg alone rather
    // than reach for the surface the *other* foot is on.
    let Some(foot_ground) = foot_ground else {
        return 0.0;
    };
    let (Some((hip, knee, ankle)), Some(pelvis)) = (chain_joints, input.joints.pelvis) else {
        return 0.0;
    };
    let (Some((hip_pos, hip_rot)), Some((knee_pos, knee_rot))) =
        (joint_pose(input.world, hip), joint_pose(input.world, knee))
    else {
        return 0.0;
    };
    let (Some((ankle_pos, _ankle_rot)), Some((_pelvis_pos, pelvis_rot))) = (
        joint_pose(input.world, ankle),
        joint_pose(input.world, pelvis),
    ) else {
        return 0.0;
    };

    // Where the ankle should go: its current (keyframe) position, raised or lowered by
    // how much *higher or lower* the ground is under this foot than under the avatar's
    // own root. On a ramp or a hillside, one foot goes up and the other down; on a flat
    // surface, both stay exactly where the animation put them.
    //
    // The reference instead aims the ankle at the ground's **absolute** height. That
    // works there because its root is planted at the *pelvis*
    // (`root_pos.z = ground + mPelvisToFoot`), so "ankle to ground" is a small
    // second-order correction under an already-correctly-standing avatar. This viewer's
    // body root sits at the **sole** instead (`body_root_transform` drops the reported
    // position by the pelvis height, and the skeleton puts `mFootLeft` at z ≈ 0.006 and
    // `mAnkleLeft` at 0.067 above it). Aiming *our* ankle at the absolute ground would
    // therefore drive it 6.7 cm too low and bury the foot — the two viewers' roots are
    // simply in different places, and this is the same correction expressed in ours.
    //
    // It has a second virtue: because the correction is a *difference* of two ground
    // samples, an error in the simulator-reported avatar position cancels out of it
    // entirely, rather than being absorbed into the legs as a permanent crouch or tiptoe.
    let ankle_world = input.root.transform_point(ankle_pos);
    let goal = to_local_point(
        input.root,
        Vec3::new(ankle_world.x, ankle_world.y + displacement, ankle_world.z),
    );

    // Keep the goal inside the leg's reach. Without this, a foot the leg cannot quite
    // stretch down to makes the solver do what the reference does — straighten the leg and
    // *point* it at the goal — which lands the ankle short of the goal and, crucially, at a
    // different horizontal position. Since the ground is sampled under the ankle, that
    // moved the probe, which moved the goal, which moved the ankle: a limit cycle that buzzed
    // the knees on every slope. Clamping the goal means the solve always reaches it exactly,
    // so the ankle stays where the probe expects it and the loop cannot close.
    //
    // See [`clamp_goal_to_reach`].
    let goal = clamp_goal_to_reach(hip_pos, knee_pos, ankle_pos, goal);

    let solved = JointSolver::new(LEG_POLE_VECTOR)
        .with_b_axis(knee_axis)
        .solve(&Chain {
            a_pos: hip_pos,
            b_pos: knee_pos,
            c_pos: ankle_pos,
            goal,
            a_rot: hip_rot,
            b_rot: knee_rot,
            a_parent_rot: pelvis_rot,
        });

    // Back from world rotations to the local rotations the pose is keyed by, each
    // against its *solved* parent so the chain composes correctly.
    let hip_local = pelvis_rot.inverse().mul_quat(solved.a_rot);
    let knee_local = solved.a_rot.inverse().mul_quat(solved.b_rot);

    // The ankle rolls flat onto the surface: keep the foot pointing where the solved leg
    // points it, but lie its sole on the ground — the true hit normal, so the foot
    // conforms to a prim ramp's face as readily as to a hillside.
    let ankle_keyframe = pose.rotation(ankle).unwrap_or(Quat::IDENTITY);
    let ankle_rot = solved.b_rot.mul_quat(ankle_keyframe);
    let up = to_local_dir(input.root, foot_ground.normal);
    let forward = ankle_rot.mul_vec3(Vec3::X);
    let ankle_local = solved.b_rot.inverse().mul_quat(basis_from_up(forward, up));

    blend_into(pose, hip, hip_local, weight);
    blend_into(pose, knee, knee_local, weight);
    blend_into(pose, ankle, ankle_local, weight);
    displacement
}

/// Blend joint `index`'s pose rotation from its keyframe rotation toward the solved
/// `target` local rotation by `weight` — at 0 the animation's own pose survives
/// untouched, at 1 the solve replaces it.
fn blend_into(pose: &mut AnimationPose, index: usize, target: Quat, weight: f32) {
    let keyframe = pose.rotation(index).unwrap_or(Quat::IDENTITY);
    pose.set_rotation(index, keyframe.slerp(target, weight.clamp(0.0, 1.0)));
}

/// The live diagnostic (env `SL_VIEWER_LOG_LOCOMOTION_IK=1`): whether to log each
/// adjusted avatar's servo state, so a live run can see the published walk speed, the
/// fly bank and the foot-IK weight rather than only their visual effect.
#[must_use]
pub(crate) fn log_enabled() -> bool {
    std::env::var("SL_VIEWER_LOG_LOCOMOTION_IK").as_deref() == Ok("1")
}

#[cfg(test)]
mod tests {
    use super::{
        ANIM_SPEED_MAX, AvatarFrame, MAX_ROLL, basis_from_up, clamp_goal_to_reach, clamp_rescale,
        fly_roll_target, walk_speed_multiplier,
    };
    use bevy::prelude::*;
    use sl_client_bevy::RegionHandle;

    /// Absolute-difference float check (the workspace forbids bare `==` on floats).
    fn near(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() <= eps, "{a} not within {eps} of {b}");
    }

    /// `clamp_rescale` maps and clamps in whichever order the output range is given —
    /// the fly bank's ramp runs *downwards*, from 0 to a negative roll.
    #[test]
    fn clamp_rescale_maps_and_clamps_either_way() {
        near(clamp_rescale(0.5, 0.0, 1.0, 0.0, 10.0), 5.0, 1e-6);
        near(clamp_rescale(-3.0, 0.0, 1.0, 0.0, 10.0), 0.0, 1e-6);
        near(clamp_rescale(9.0, 0.0, 1.0, 0.0, 10.0), 10.0, 1e-6);
        // Descending output range (the fly bank's speed ramp).
        near(clamp_rescale(3.0, 7.0, 15.0, 0.0, -MAX_ROLL), 0.0, 1e-6);
        near(
            clamp_rescale(20.0, 7.0, 15.0, 0.0, -MAX_ROLL),
            -MAX_ROLL,
            1e-6,
        );
        near(
            clamp_rescale(11.0, 7.0, 15.0, 0.0, -MAX_ROLL),
            -MAX_ROLL / 2.0,
            1e-6,
        );
        // A zero-width input range collapses to the low output.
        near(clamp_rescale(5.0, 2.0, 2.0, 3.0, 9.0), 3.0, 1e-6);
    }

    /// The walk servo speeds the cycle up when the planted foot is sliding forward
    /// (the animation is too slow for the ground speed) and slows it when the foot
    /// outruns the avatar — the whole point of `LLWalkAdjustMotion`.
    #[test]
    fn walk_servo_matches_foot_speed_to_ground_speed() {
        // A foot planted perfectly: its ground speed already equals the avatar's, so
        // the cycle plays at its authored rate.
        near(walk_speed_multiplier(3.0, 3.0), 1.0, 1e-6);
        // The avatar is moved faster than the cycle can carry it, so the foot only
        // "grips" at half the ground speed: play the cycle twice as fast — capped.
        near(
            walk_speed_multiplier(3.0, 1.5),
            2.0_f32.min(ANIM_SPEED_MAX),
            1e-6,
        );
        // The foot outruns the avatar (the cycle is too fast): slow it down.
        assert!(walk_speed_multiplier(2.0, 4.0) < 1.0);
        // A foot with no grip at all saturates rather than dividing by zero.
        near(walk_speed_multiplier(3.0, 0.0), ANIM_SPEED_MAX, 1e-6);
        // A barely-moving avatar still gets a small floor, never a dead stop.
        assert!(walk_speed_multiplier(0.2, 100.0) > 0.0);
    }

    /// `basis_from_up` lies `+Z` on the given up vector while keeping `+X` as near the
    /// requested forward as orthogonality allows — the ankle's roll onto a slope.
    #[test]
    fn basis_from_up_aligns_z_and_keeps_forward() {
        // A ~30° slope rising to the east.
        let up = Vec3::new(-0.5, 0.0, 3.0_f32.sqrt() / 2.0).normalize();
        let rotation = basis_from_up(Vec3::X, up);
        near(rotation.mul_vec3(Vec3::Z).dot(up), 1.0, 1e-5);
        // The foot still points broadly forward, just tilted onto the slope, and its
        // forward axis now lies *in* the slope.
        assert!(rotation.mul_vec3(Vec3::X).dot(Vec3::X) > 0.8);
        near(rotation.mul_vec3(Vec3::X).dot(up), 0.0, 1e-5);
        // A degenerate forward (parallel to up) yields the identity rather than NaNs.
        assert!(basis_from_up(up, up).abs_diff_eq(Quat::IDENTITY, 1e-6));
    }

    /// The fly bank only engages above 7 m/s, saturates at 15, and its sign follows the
    /// turn direction. Flying fast but straight never banks.
    #[test]
    fn fly_bank_scales_in_with_speed_and_follows_the_turn() {
        // Too slow to bank at all, however hard the avatar turns.
        near(fly_roll_target(4.0, 3.0), 0.0, 1e-6);
        // At full speed the bank is the clamped yaw rate times the full roll authority,
        // and opposite turns bank opposite ways.
        let left = fly_roll_target(4.0, 20.0);
        let right = fly_roll_target(-4.0, 20.0);
        near(left, -MAX_ROLL * 4.0, 1e-5);
        near(right, MAX_ROLL * 4.0, 1e-5);
        // The yaw rate is clamped, so a spin does not bank the avatar past the limit.
        near(fly_roll_target(50.0, 20.0), left, 1e-5);
        // Straight and level: no bank.
        near(fly_roll_target(0.0, 20.0), 0.0, 1e-6);
    }

    /// A goal needing no correction is left exactly alone, even though a standing leg sits
    /// within the reach margin of full extension. Getting this wrong tugged both ankles up
    /// toward the hips and visibly lifted the avatar every time it stopped walking — the
    /// foot IK must be a *perfect* no-op on flat ground.
    #[test]
    fn reach_clamp_never_shortens_an_uncorrected_leg() {
        // A near-straight standing leg: thigh 0.5 + shin 0.5, ankle 0.998 below the hip —
        // 99.8% extended, well inside the 0.5% margin.
        let hip = Vec3::ZERO;
        let knee = Vec3::new(0.03, 0.0, -0.5);
        let ankle = Vec3::new(0.0, 0.0, -0.998);
        // Zero ground displacement ⟹ the goal *is* the ankle. It must survive untouched.
        let goal = clamp_goal_to_reach(hip, knee, ankle, ankle);
        assert!(
            goal.abs_diff_eq(ankle, 1e-6),
            "an uncorrected foot was moved to {goal:?}, away from its pose at {ankle:?}",
        );
    }

    /// A goal beyond the leg's reach is pulled back onto it, so the solve lands the ankle
    /// *on* the goal rather than straightening the leg and pointing it at one it cannot
    /// touch — the horizontal slip that closed the jitter feedback loop.
    #[test]
    fn reach_clamp_pulls_an_unreachable_goal_onto_the_leg() {
        // A visibly bent leg, so the posed extension (0.9 m) is short of the margin-backed
        // full reach and the margin — not the pose — is what does the limiting.
        let hip = Vec3::ZERO;
        let knee = Vec3::new(0.15, 0.0, -0.45);
        let ankle = Vec3::new(0.0, 0.0, -0.9);
        let full = knee.distance(hip) + ankle.distance(knee);
        // A foot asked to reach 1.4 m down: far beyond the leg.
        let goal = clamp_goal_to_reach(hip, knee, ankle, Vec3::new(0.0, 0.0, -1.4));
        let reach = goal.distance(hip);
        // Pulled back to just inside full extension — never past it, never onto it.
        near(reach, full * (1.0 - super::LEG_REACH_MARGIN), 1e-5);
        assert!(
            reach < full && reach > 0.9,
            "clamped goal {reach} should sit between the posed 0.9 m and the full {full} m",
        );
        // …and along the direction it was asked to reach in.
        near(goal.normalize().dot(Vec3::NEG_Z), 1.0, 1e-5);
    }

    /// The skeleton → region conversion carries the avatar's yaw: a point 1 m in *front*
    /// of an avatar facing north (+Y) is 1 m north of it. This is the frame the walk
    /// servo differences its foot positions in.
    #[test]
    fn avatar_frame_applies_the_yaw() {
        let frame = AvatarFrame {
            origin: Vec3::new(128.0, 64.0, 21.5),
            // Facing north: Second Life `+X` (forward) turned 90° about up onto `+Y`.
            rotation: Quat::from_rotation_z(core::f32::consts::FRAC_PI_2),
            region: RegionHandle::new(0),
        };
        let ahead = frame.to_region(Vec3::X);
        assert!(ahead.abs_diff_eq(Vec3::new(128.0, 65.0, 21.5), 1e-5));
        // A direction carries the rotation but not the origin: the avatar's own forward
        // (+X) is north (+Y) in the region, and up stays up.
        assert!(frame.dir_to_local(Vec3::Y).abs_diff_eq(Vec3::X, 1e-5));
        assert!(frame.dir_to_local(Vec3::Z).abs_diff_eq(Vec3::Z, 1e-5));
    }
}
