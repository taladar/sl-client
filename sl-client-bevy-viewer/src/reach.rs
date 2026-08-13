//! Activity-driven reach & aim (P31.15): the two procedural adjusters the reference
//! viewer drives from what the avatar is *doing* rather than from how it is moving —
//! `LLEditingMotion` and `LLTargetingMotion`. Both layer onto the sampled keyframe
//! pose at the same seam the look-at (P31.12) and locomotion (P31.14) folds use, and
//! the reach reuses P31.14's [`JointSolver`] unchanged.
//!
//! - **`LLEditingMotion`** (`lleditingmotion.cpp`) reaches the **left** arm toward the
//!   object the avatar has selected: a two-bone IK solve on
//!   `mShoulderLeft → mElbowLeft → mWristLeft`, whose end effector is not the wrist but
//!   a point [`WRIST_OFFSET`] beyond it (roughly the hand), so the *hand* lands on the
//!   target rather than the joint. It also asks for the [`EDITING_HAND_POSE`] hand shape
//!   (through the P31.13 morph pipeline) and, at the reference's `HIGH_PRIORITY`,
//!   straightens the torso — see [`apply_editing`].
//! - **`LLTargetingMotion`** (`lltargetingmotion.cpp`) twists the torso (an *additive*
//!   rotation, constrained to [`TORSO_ROTATION_CONSTRAINT`]) until the avatar's **right**
//!   hand points along its look-at direction. It runs only while one of the four
//!   `AGENT_GUN_AIM_ANIMS` is signalled ([`sl_anim::is_gun_aim_trigger`]) — an avatar
//!   holding a weapon at rest aims at nothing.
//!
//! # The state the viewer did not track
//!
//! Both motions are driven by a *target*, and the reference gets each from a HUD effect
//! rather than from the animation stream:
//!
//! - The editing target is the **point-at** effect (`LLHUDEffectPointAt`): the own
//!   avatar publishes what it has selected, and every viewer that receives one starts the
//!   editing motion on the *source* avatar. So this module adds both halves — an own
//!   [`PointAtSelection`] (press [`SELECT_KEY`] to select the object under the crosshair,
//!   press it aiming at nothing to deselect), which is published to the simulator as a
//!   `ViewerEffect` so other viewers see the reach, and the receive path that turns other
//!   avatars' point-at effects into [`PointAtTargets`].
//! - The aim target is the **look-at** point, which [`crate::look_at`] (P31.12) already
//!   tracks for every avatar — the own avatar's is the debug fly-camera's position (the
//!   documented stand-in for the reference's mouselook focus), so an aiming own avatar
//!   tracks the camera.
//!
//! The reference resolves a point-at whose target is an *object* against that object's
//! current transform every frame, so the reach follows a moving target; so does this
//! ([`drive_own_point_at`] re-reads the selected entity's global each frame, and
//! [`receive_point_at_effects`] resolves the on-wire offset against the target object's
//! entity when it knows it — the wire field is a *local offset* when a target object is
//! set and a global position only when it is not).
//!
//! # Debugging
//!
//! - [`SELECT_KEY`] (`E`) selects / deselects the object under the crosshair, engaging
//!   the reach.
//! - [`AIM_KEY`] (`G`) starts / stops `aim_rifle_r` on the own avatar through the
//!   simulator (`AgentAnimation`), which comes back as an ordinary signalled animation
//!   and switches the targeting motion on through the normal path — no test-only state.
//! - `SL_VIEWER_LOG_REACH=1` logs each avatar's editing / aiming weights, how far off its goal
//!   the solved arm still points, and how far off its target the twisted torso leaves the
//!   right hand aiming — the two self-checks that say each solve actually converged.

use std::collections::HashMap;

use bevy::prelude::*;
use sl_anim::{HandPose, JointPriority};
use sl_client_bevy::{
    AgentKey, AnimationKey, AnimationPose, Command, GlobalCoordinates, ObjectKey, PointAtType,
    RegionHandle, SlCommand, SlEvent, SlIdentity, SlSessionEvent, Uuid, ViewerEffect,
    ViewerEffectData, ViewerEffectType,
};

use crate::camera::ViewerCamera;
use crate::coords::{metres_to_f32, sl_to_bevy_vec};
use crate::ik::{Chain, JointSolver};
use crate::locomotion_ik::clamp_rescale;
use crate::look_at::{basis_rotation, constrain, smooth_interpolant};
use crate::objects::{ObjectState, SceneObject};

/// The key that selects the object under the crosshair as the editing target (and, aimed
/// at nothing, clears the selection).
const SELECT_KEY: KeyCode = KeyCode::KeyE;

/// The key that starts / stops the own avatar's aim animation, exercising the targeting
/// motion end to end (the simulator echoes it back as a signalled animation).
const AIM_KEY: KeyCode = KeyCode::KeyG;

/// The built-in animation [`AIM_KEY`] plays — one of the reference's
/// `AGENT_GUN_AIM_ANIMS`, so it activates the targeting motion.
const AIM_ANIMATION: &str = "aim_rifle_r";

/// Half-life (seconds) of the editing reach's smoothing toward its solved arm pose
/// (reference `TARGET_LAG_HALF_LIFE`).
const EDIT_TARGET_LAG_HALF_LIFE: f32 = 0.1;

/// Half-life (seconds) of the targeting motion's smoothing toward its ideal torso twist
/// (reference `TORSO_TARGET_HALF_LIFE`).
const TORSO_TARGET_HALF_LIFE: f32 = 0.25;

/// Seconds the editing reach takes to fade back out of the pose once the selection is
/// dropped (reference `EDITING_EASEOUT_DURATION`; its ease-*in* is `0`, so the reach
/// engages on the frame the target appears).
const EDITING_EASE_OUT: f32 = 0.5;

/// Seconds the targeting twist takes to fade in (reference `TARGETING_EASEIN_DURATION`).
const TARGETING_EASE_IN: f32 = 0.3;

/// Seconds the targeting twist takes to fade out (reference `TARGETING_EASEOUT_DURATION`).
const TARGETING_EASE_OUT: f32 = 0.5;

/// The end effector of the editing chain sits this far beyond the wrist, in the elbow's
/// frame — the reference's `mWristOffset`, which puts the *hand* on the target instead of
/// the wrist joint.
const WRIST_OFFSET: Vec3 = Vec3::new(0.0, 0.2, 0.0);

/// The editing chain's pole vector, in the shoulder parent's (`mCollarLeft`'s) frame:
/// where the elbow points (reference `mIKSolver.setPoleVector`).
const EDIT_POLE_VECTOR: Vec3 = Vec3::new(-1.0, 1.0, 0.0);

/// The left elbow's hinge axis in its own frame (reference `mIKSolver.setBAxis`), which
/// the reference notes is limb-specific and keeps the arm out of the singular
/// configurations a derived bend plane would fall into.
const ELBOW_BEND_AXIS: Vec3 = Vec3::new(-0.682_683, 0.0, -0.730_714);

/// The normal of the reference's "edit plane" in the torso's frame: a target *behind* this
/// plane is folded and lifted (see [`edit_goal`]) so the avatar reaches over itself rather
/// than through itself (reference `edit_plane_normal`, `(1, 1, 0)` normalised).
const EDIT_PLANE_NORMAL: Vec3 = Vec3::new(
    core::f32::consts::FRAC_1_SQRT_2,
    core::f32::consts::FRAC_1_SQRT_2,
    0.0,
);

/// How far up (metres) a target fully behind the edit plane is lifted as it is folded, so a
/// target directly behind the avatar becomes an overhead reach rather than a backwards one
/// (reference `clamp_rescale(dot, 0.f, -1.f, 0.f, 5.f)`).
const EDIT_PLANE_LIFT: f32 = 5.0;

/// Limit angle (radians) of the targeting motion's total torso rotation (reference
/// `total_rot.constrain(F_PI_BY_TWO * 0.8f)`).
const TORSO_ROTATION_CONSTRAINT: f32 = core::f32::consts::FRAC_PI_2 * 0.8;

/// The direction the right hand "points" in its own frame — the reference's
/// `LLVector3(0.f, -1.f, 0.f) * mRightHandJoint->getWorldRotation()`, i.e. the axis a
/// held weapon's barrel runs along.
const RIGHT_HAND_AIM_AXIS: Vec3 = Vec3::new(0.0, -1.0, 0.0);

/// Minimum distance (metres) from the shoulder's parent to the editing target before the
/// reach engages: a target inside the shoulder has no direction to solve toward.
const MIN_EDIT_TARGET_DISTANCE: f32 = 0.05;

/// The hand shape the editing motion asks for while it reaches (reference
/// `LLEditingMotion::sHandPose`), driven through the P31.13 hand-pose morph pipeline.
pub(crate) const EDITING_HAND_POSE: HandPose = HandPose::RELAXED_R;

/// The priority the editing motion's hand-pose request carries (reference
/// `LLEditingMotion::sHandPosePriority`, `3` = `LLJoint::HIGHER_PRIORITY`), so a
/// higher-priority animation still owns the hands.
pub(crate) const EDITING_HAND_POSE_PRIORITY: JointPriority = JointPriority::HIGHER;

/// Seconds a received point-at target stays valid before it is pruned and the arm
/// relaxes. The reference resends an active effect periodically; this outlives the
/// resend interval ([`POINT_AT_RESEND_INTERVAL`]) but drops a stale one.
const POINT_AT_TARGET_TTL: f32 = 3.0;

/// Seconds between re-sends of the own avatar's point-at effect while a selection stands,
/// so other viewers keep seeing the reach (the reference's `LLHUDEffect` re-send cadence).
const POINT_AT_RESEND_INTERVAL: f32 = 1.0;

/// The lifetime advertised on the point-at effect sent to other viewers.
const POINT_AT_EFFECT_DURATION: f32 = 3.0;

/// The colour advertised on the point-at effect (the reference sends the user's effect
/// colour; nothing renders it here, and the arm reach it triggers is colourless).
const POINT_AT_EFFECT_COLOR: [u8; 4] = [255, 255, 255, 255];

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

/// One avatar's point-at target: a point in Bevy world space and the time (seconds since
/// startup) it expires. The own avatar's is refreshed every frame from its selection, so
/// it never expires while the selection stands.
struct PointAtTarget {
    /// The point-at point in Bevy world space.
    point: Vec3,
    /// Elapsed-seconds timestamp after which the target is pruned.
    expires_at: f32,
}

/// The current world point-at target of each avatar that has one, keyed by agent — the
/// editing motion's target. Populated by [`drive_own_point_at`] (own avatar, from its
/// selection) and [`receive_point_at_effects`] (others, from `ViewerEffect`), and read by
/// the pose fold and the hand-pose driver (an avatar reaching for something also holds
/// its hand in [`EDITING_HAND_POSE`]).
#[derive(Resource, Default)]
pub(crate) struct PointAtTargets {
    /// Per-avatar point-at points, keyed by agent id.
    targets: HashMap<AgentKey, PointAtTarget>,
}

impl PointAtTargets {
    /// The Bevy-world point-at point for `agent`, if it currently has one.
    #[must_use]
    pub(crate) fn point(&self, agent: AgentKey) -> Option<Vec3> {
        self.targets.get(&agent).map(|target| target.point)
    }

    /// Whether `agent`'s editing motion is active — the reference starts
    /// `ANIM_AGENT_EDITING` exactly when the avatar has a point-at target.
    #[must_use]
    pub(crate) fn is_editing(&self, agent: AgentKey) -> bool {
        self.targets.contains_key(&agent)
    }

    /// Record `point` (Bevy world space) as `agent`'s point-at target, valid until
    /// `now + POINT_AT_TARGET_TTL`.
    fn set(&mut self, agent: AgentKey, point: Vec3, now: f32) {
        self.targets.insert(
            agent,
            PointAtTarget {
                point,
                expires_at: now + POINT_AT_TARGET_TTL,
            },
        );
    }

    /// Drop `agent`'s point-at target (a cleared selection / ended gesture).
    fn clear(&mut self, agent: AgentKey) {
        self.targets.remove(&agent);
    }

    /// Prune every target whose expiry has passed at `now`.
    fn prune(&mut self, now: f32) {
        self.targets
            .retain(|_agent, target| target.expires_at > now);
    }
}

/// The own avatar's current selection — the object it is "editing", which drives its
/// point-at target and hence the editing reach. The reference's `LLSelectMgr` selection,
/// reduced to the one thing the motion needs: which object, and where on it.
#[derive(Resource, Default)]
pub(crate) struct PointAtSelection {
    /// The selected object, or [`None`] when nothing is selected.
    selected: Option<Selection>,
    /// The last `(object, type)` advertised to the simulator and when, so the effect is
    /// re-sent on a change and periodically while it stands, rather than every frame.
    advertised: Option<(ObjectKey, f32)>,
}

/// One selected object: the entity to read the live transform from (so the reach follows
/// a moving object), its grid-wide key (what goes on the wire), and the point on it that
/// was picked, in the object's own Second Life frame.
#[derive(Clone, Copy)]
struct Selection {
    /// The object's entity, re-read each frame for its current world transform.
    entity: Entity,
    /// The object's grid-wide key, sent as the effect's target object.
    key: ObjectKey,
    /// The picked point as an offset in the object's own frame — the wire format's
    /// `TargetPos` when a target object is set.
    offset: Vec3,
}

/// One avatar's cross-frame reach / aim state: the smoothed arm pose the editing reach
/// lags toward, the smoothed additive torso twist of the targeting motion, and the eased
/// weight of each (the reference's motion ease-in / ease-out).
#[derive(Default)]
struct AgentReach {
    /// The eased `[0, 1]` weight of the editing reach.
    edit_weight: f32,
    /// The previous frame's applied left-shoulder local rotation, which the target lag
    /// eases away from (reference: the lag reads the joint's current rotation, i.e. the
    /// previous frame's blended output).
    last_shoulder: Option<Quat>,
    /// The previous frame's applied left-elbow local rotation.
    last_elbow: Option<Quat>,
    /// The eased `[0, 1]` weight of the targeting twist.
    aim_weight: f32,
    /// The smoothed **additive** torso rotation the targeting motion applies (reference
    /// `mTorsoState`'s rotation, which its `nlerp` reads back each frame).
    torso_delta: Quat,
}

/// Per-avatar [`AgentReach`] state, keyed by agent. Retained across frames so the arm's
/// target lag and the torso's twist are continuous.
#[derive(Resource, Default)]
pub(crate) struct ReachMotion {
    /// The reach / aim state of each rigged avatar seen so far.
    states: HashMap<AgentKey, AgentReach>,
}

impl ReachMotion {
    /// Forget every avatar not in `live` (they have despawned).
    pub(crate) fn retain(&mut self, live: &impl Fn(AgentKey) -> bool) {
        self.states.retain(|&agent, _state| live(agent));
    }
}

/// The skeleton indices of the joints the reach / aim adjusters drive, resolved once per
/// avatar per frame. A skeleton missing one simply does not run that motion.
#[derive(Clone, Copy, Default)]
pub(crate) struct ReachJoints {
    /// `mCollarLeft` — the left shoulder's parent, whose frame the pole vector and the
    /// edit target are expressed in (the reference takes it as
    /// `getJoint("mShoulderLeft")->getParent()`).
    pub(crate) collar: Option<usize>,
    /// `mShoulderLeft` / `mElbowLeft` / `mWristLeft` — the editing reach's chain.
    pub(crate) shoulder: Option<usize>,
    /// See [`shoulder`](Self::shoulder).
    pub(crate) elbow: Option<usize>,
    /// See [`shoulder`](Self::shoulder).
    pub(crate) wrist: Option<usize>,
    /// `mTorso` — straightened by the editing motion, twisted by the targeting one.
    pub(crate) torso: Option<usize>,
    /// `mWristRight` — the joint the targeting motion aims at the look-at target.
    pub(crate) right_wrist: Option<usize>,
}

impl ReachJoints {
    /// Resolve the joint indices from a skeleton's name lookup.
    pub(crate) fn resolve(joint_index: impl Fn(&str) -> Option<usize>) -> Self {
        Self {
            collar: joint_index("mCollarLeft"),
            shoulder: joint_index("mShoulderLeft"),
            elbow: joint_index("mElbowLeft"),
            wrist: joint_index("mWristLeft"),
            torso: joint_index("mTorso"),
            right_wrist: joint_index("mWristRight"),
        }
    }
}

/// Everything one avatar's reach / aim fold needs from outside this module for one frame.
pub(crate) struct ReachInput<'a> {
    /// The avatar being adjusted.
    pub(crate) agent: AgentKey,
    /// Every joint's deformed **world** matrix in the avatar-local Second Life frame,
    /// with the keyframe + idle + look-at pose already folded in — the pose these
    /// adjusters correct, and where the IK reads the current arm geometry from.
    pub(crate) world: &'a [Mat4],
    /// The avatar body root's Bevy global, which carries the avatar-local Second Life
    /// frame into the world (so a world target can be brought back into it).
    pub(crate) root: &'a GlobalTransform,
    /// The skeleton indices of the joints the adjusters drive.
    pub(crate) joints: ReachJoints,
    /// The avatar's point-at target in Bevy world space (its selection), if any — the
    /// editing reach's goal.
    pub(crate) point_at: Option<Vec3>,
    /// The avatar's look-at target in Bevy world space, if any — the targeting motion's
    /// aim direction (P31.12 already tracks it for every avatar).
    pub(crate) look_at: Option<Vec3>,
    /// Whether one of the `AGENT_GUN_AIM_ANIMS` is signalled, which is what switches the
    /// targeting motion on.
    pub(crate) aiming: bool,
    /// The frame time, seconds.
    pub(crate) dt: f32,
}

/// What the reach / aim fold did to one avatar this frame, for the live diagnostic
/// (`SL_VIEWER_LOG_REACH=1`).
#[derive(Clone, Copy, Default)]
pub(crate) struct ReachReport {
    /// The eased weight the editing reach is applied at.
    pub(crate) edit_weight: f32,
    /// How far off the goal the solved arm still **points**, radians: the angle at the
    /// shoulder between the hand it produced and the goal. Near zero whenever the IK solved,
    /// whether or not the goal was in reach — a goal beyond the arm (any object more than
    /// arm's length away, i.e. most of them) leaves the hand metres short of it by design,
    /// so a *distance* here would report a working solve as a large error.
    pub(crate) point_error: f32,
    /// The eased weight the targeting twist is applied at.
    pub(crate) aim_weight: f32,
    /// The angle (radians) of the additive torso twist actually applied.
    pub(crate) torso_twist: f32,
    /// How far off its target the right hand still aims once the twist is applied, radians
    /// (see [`aim_residual`]) — ~0 when the aim solved, and the leftover when the torso
    /// limit could not cover the turn.
    pub(crate) aim_residual: f32,
    /// The aim direction in the avatar's own frame (Second Life `+X` is forward), so a
    /// target behind the avatar is visible as a negative `x` in the log.
    pub(crate) aim_dir: Vec3,
}

/// The world position and rotation of joint `index` in the avatar-local Second Life
/// frame, read out of the deformed world matrices.
fn joint_pose(world: &[Mat4], index: Option<usize>) -> Option<(Vec3, Quat)> {
    let matrix = world.get(index?)?;
    let (_scale, rotation, translation) = matrix.to_scale_rotation_translation();
    Some((translation, rotation))
}

/// Blend joint `index`'s pose rotation from its keyframe rotation toward `target` by
/// `weight` — at 0 the animation's own pose survives untouched, at 1 the motion's pose
/// replaces it (both these motions *replace* rather than layer on the joints they drive;
/// the targeting motion's additive torso twist is the one exception and is composed onto
/// the keyframe explicitly).
fn blend_into(pose: &mut AnimationPose, index: Option<usize>, target: Quat, weight: f32) {
    if let Some(index) = index {
        let keyframe = pose.rotation(index).unwrap_or(Quat::IDENTITY);
        pose.set_rotation(index, keyframe.slerp(target, weight.clamp(0.0, 1.0)));
    }
}

/// Ease a motion weight toward `on` / off, over the reference's ease-in / ease-out
/// durations. A zero duration engages (or drops) on the frame itself.
fn ease(weight: f32, on: bool, dt: f32, ease_in: f32, ease_out: f32) -> f32 {
    let step = |duration: f32| if duration > 0.0 { dt / duration } else { 1.0 };
    if on {
        (weight + step(ease_in)).clamp(0.0, 1.0)
    } else {
        (weight - step(ease_out)).clamp(0.0, 1.0)
    }
}

/// Where the editing reach actually aims, given the target the avatar selected.
///
/// `LLEditingMotion::onUpdate`'s target fold, and the reason an avatar editing something
/// behind itself reaches *up and over* rather than trying to pass its arm through its own
/// chest. The target **direction** (from the shoulder's parent; the *distance* is
/// preserved throughout) is left alone while it lies in front of the "edit plane" — a
/// plane through the torso, at 45° to its forward, that rides with it — and folded when it
/// lies behind:
///
/// - its component along the plane normal is amplified (the reference's
///   `target + normal * (dot * 2)`, with `dot` negative), and
/// - it is lifted by up to [`EDIT_PLANE_LIFT`] metres, in proportion to how far behind the
///   plane it was, before being re-normalised.
///
/// The lift dominates: a target directly behind the avatar comes out pointing steeply
/// **upward**, which is the whole point — the arm has nowhere else to go. Note that the
/// first step is *not* a mirror (a reflection would be `target - normal * (dot * 2)`); it
/// pushes the direction further off the plane rather than across it, and only the lift
/// makes the result reachable. Ported as it behaves, as with the other vestigial reference
/// details in [`crate::locomotion_ik`]: the visible motion is what a viewer must match.
fn edit_goal(parent_pos: Vec3, torso_world: Quat, focus: Vec3) -> Vec3 {
    let to_target = vsub(focus, parent_pos);
    let distance = to_target.length();
    let direction = to_target.normalize_or_zero();
    let plane_normal = torso_world.mul_vec3(EDIT_PLANE_NORMAL).normalize_or_zero();
    let dot = plane_normal.dot(direction);
    let direction = if dot < 0.0 {
        let folded = vadd(direction, vscale(plane_normal, dot * 2.0));
        let lifted = Vec3::new(
            folded.x,
            folded.y,
            folded.z + clamp_rescale(dot, 0.0, -1.0, 0.0, EDIT_PLANE_LIFT),
        );
        lifted.normalize_or_zero()
    } else {
        direction
    };
    vadd(parent_pos, vscale(direction, distance))
}

/// The **additive** torso rotation that turns the avatar's right hand onto its aim
/// direction — `LLTargetingMotion::onUpdate`'s ideal twist, before smoothing and the
/// rotation limit.
///
/// Both aims are expressed as a full orientation (`+X` along the direction, `+Z` roughly
/// up): the delta between the hand's and the target's, carried into the torso's own frame
/// so it can be composed onto the torso's keyframe rotation. A target straight up or down
/// leaves the basis undefined and yields no twist.
fn torso_aim_delta(aim_dir: Vec3, torso_world: Quat, right_hand_world: Quat) -> Quat {
    let target = aim_dir.normalize_or_zero();
    let hand_at = right_hand_world
        .mul_vec3(RIGHT_HAND_AIM_AXIS)
        .normalize_or_zero();
    // Either direction parallel to the up axis degenerates the (up × dir) basis.
    if Vec3::Z.cross(target).length_squared() < f32::EPSILON
        || Vec3::Z.cross(hand_at).length_squared() < f32::EPSILON
    {
        return Quat::IDENTITY;
    }
    let target_aim = basis_rotation(target);
    let hand_aim = basis_rotation(hand_at);
    // Linden's `(cur_torso * ~right_hand_rot) * target_aim_rot * ~cur_torso`, mirrored
    // into glam's opposite composition order: the rotation taking the hand's aim onto the
    // target's, conjugated into the torso's frame.
    let ideal = target_aim
        .mul_quat(hand_aim.inverse())
        .mul_quat(torso_world);
    torso_world.inverse().mul_quat(ideal)
}

/// `LLEditingMotion`: solve the left arm onto the point-at target and fold the result into
/// `pose`, returning how far off the goal the solved arm still points (radians; see
/// [`ReachReport::point_error`]).
///
/// Three details worth naming:
///
/// - The end effector is the wrist displaced by [`WRIST_OFFSET`] in the elbow's frame, not
///   the wrist joint, so it is the *hand* that arrives on the object.
/// - The solved arm is **lagged** toward the previous frame's applied pose
///   ([`EDIT_TARGET_LAG_HALF_LIFE`]) rather than snapped, so a moving target does not make
///   the arm judder. The reference lags against the joint's current rotation — which *is*
///   the previous frame's blended output — so the per-avatar copy kept here is the same
///   quantity, read from a frame the fold owns rather than from the skeleton it is about
///   to overwrite.
/// - The torso is driven to **rest** while reaching. That is not an embellishment: the
///   reference adds `mTorsoState` to the editing motion's pose with `ROT` usage and never
///   assigns it a rotation, so an identity torso rotation is blended in at the motion's
///   `HIGH_PRIORITY` — the avatar straightens up as it reaches. Ported as it behaves.
fn apply_editing(
    pose: &mut AnimationPose,
    input: &ReachInput<'_>,
    state: &mut AgentReach,
    focus: Vec3,
) -> f32 {
    let joints = input.joints;
    let (Some((parent_pos, parent_rot)), Some((shoulder_pos, shoulder_rot))) = (
        joint_pose(input.world, joints.collar),
        joint_pose(input.world, joints.shoulder),
    ) else {
        return 0.0;
    };
    let (Some((elbow_pos, elbow_rot)), Some((wrist_pos, _wrist_rot))) = (
        joint_pose(input.world, joints.elbow),
        joint_pose(input.world, joints.wrist),
    ) else {
        return 0.0;
    };
    let Some((_torso_pos, torso_rot)) = joint_pose(input.world, joints.torso) else {
        return 0.0;
    };
    if vsub(focus, parent_pos).length() < MIN_EDIT_TARGET_DISTANCE {
        return 0.0;
    }

    let effector = vadd(wrist_pos, elbow_rot.mul_vec3(WRIST_OFFSET));
    let goal = edit_goal(parent_pos, torso_rot, focus);
    let solved = JointSolver::new(EDIT_POLE_VECTOR)
        .with_b_axis(ELBOW_BEND_AXIS)
        .solve(&Chain {
            a_pos: shoulder_pos,
            b_pos: elbow_pos,
            c_pos: effector,
            goal,
            a_rot: shoulder_rot,
            b_rot: elbow_rot,
            a_parent_rot: parent_rot,
        });

    // Back from world rotations to the local ones the pose is keyed by, each against its
    // solved parent so the chain composes correctly.
    let shoulder_local = parent_rot.inverse().mul_quat(solved.a_rot);
    let elbow_local = solved.a_rot.inverse().mul_quat(solved.b_rot);

    // The target lag: ease away from the solved pose toward the previous frame's applied
    // one (the reference's `slerp(interpolant, solved, previous)`).
    let lag = smooth_interpolant(EDIT_TARGET_LAG_HALF_LIFE, input.dt);
    let shoulder_local = state.last_shoulder.map_or(shoulder_local, |previous| {
        shoulder_local.slerp(previous, lag)
    });
    let elbow_local = state
        .last_elbow
        .map_or(elbow_local, |previous| elbow_local.slerp(previous, lag));
    state.last_shoulder = Some(shoulder_local);
    state.last_elbow = Some(elbow_local);

    let weight = state.edit_weight;
    blend_into(pose, joints.shoulder, shoulder_local, weight);
    blend_into(pose, joints.elbow, elbow_local, weight);
    // The reference leaves the wrist at rest relative to the solved elbow, and straightens
    // the torso (see above).
    blend_into(pose, joints.wrist, Quat::IDENTITY, weight);
    blend_into(pose, joints.torso, Quat::IDENTITY, weight);

    // Where the hand actually ended up, for the diagnostic — as an *angle*, not a distance.
    // The solver keeps the bone lengths, so a goal beyond the arm's reach (an object across
    // the parcel, which is a perfectly ordinary thing to select) leaves the hand metres short
    // of it however well the solve went: the reference straightens the arm and *points*. What
    // says the IK worked is therefore whether the arm points **at** the goal, which is what
    // this measures.
    let solved_elbow = vadd(
        shoulder_pos,
        solved.a_rot.mul_vec3(
            shoulder_rot
                .inverse()
                .mul_vec3(vsub(elbow_pos, shoulder_pos)),
        ),
    );
    let solved_hand = vadd(
        solved_elbow,
        solved
            .b_rot
            .mul_vec3(elbow_rot.inverse().mul_vec3(vsub(effector, elbow_pos))),
    );
    vsub(solved_hand, shoulder_pos).angle_between(vsub(goal, shoulder_pos))
}

/// `LLTargetingMotion`: twist the torso until the right hand points along `aim_dir` (in the
/// avatar-local Second Life frame), and fold the result into `pose`. Returns the angle
/// (radians) of the twist actually applied.
///
/// Unlike the editing reach this is an **additive** rotation: it is composed onto whatever
/// the animation does to the torso (the reference registers the motion with
/// `ADDITIVE_BLEND`, whose blender applies `added_rot * blended_rot`), and the *total* — the
/// keyframe torso plus the twist — is what the [`TORSO_ROTATION_CONSTRAINT`] limits, so an
/// animation that already turns the torso gets a correspondingly smaller twist.
fn apply_targeting(
    pose: &mut AnimationPose,
    input: &ReachInput<'_>,
    state: &mut AgentReach,
    aim_dir: Option<Vec3>,
) -> f32 {
    let Some(torso) = input.joints.torso else {
        return 0.0;
    };
    let keyframe = pose.rotation(torso).unwrap_or(Quat::IDENTITY);
    // With an aim direction, ease the smoothed twist toward the ideal one; without (the
    // motion is easing out, or the avatar has no look-at target) hold the last twist and
    // let the weight fade it away.
    if let (Some(aim), Some((_pos, torso_world)), Some((_hand_pos, hand_world))) = (
        aim_dir,
        joint_pose(input.world, input.joints.torso),
        joint_pose(input.world, input.joints.right_wrist),
    ) {
        let ideal = torso_aim_delta(aim, torso_world, hand_world);
        let smoothed = state
            .torso_delta
            .lerp(ideal, smooth_interpolant(TORSO_TARGET_HALF_LIFE, input.dt));
        // Constrain the *total* torso rotation, then express the constrained result back as
        // an additive delta — exactly the reference's `total_rot.constrain(...)` dance.
        let total = constrain(keyframe.mul_quat(smoothed), TORSO_ROTATION_CONSTRAINT);
        state.torso_delta = keyframe.inverse().mul_quat(total);
    }
    let applied = Quat::IDENTITY.slerp(state.torso_delta, state.aim_weight.clamp(0.0, 1.0));
    pose.set_rotation(torso, keyframe.mul_quat(applied));
    Quat::IDENTITY.angle_between(applied)
}

/// How far off the target the avatar's right hand still aims after the twist, radians — the
/// self-check for the targeting motion.
///
/// The twist rotates the torso, and everything below it (including the right arm) rides
/// along, so applying the same rotation to the hand's current world rotation is where the
/// hand *will* point. A twist that solved leaves ~0 here; one clamped by
/// [`TORSO_ROTATION_CONSTRAINT`] leaves the residual the limit could not cover. This is what
/// distinguishes "the torso is turning the wrong way" from "the torso is turning as far as it
/// is allowed to".
fn aim_residual(input: &ReachInput<'_>, aim_dir: Vec3, applied: Quat) -> f32 {
    let Some((_pos, hand_world)) = joint_pose(input.world, input.joints.right_wrist) else {
        return 0.0;
    };
    let Some((_pos, torso_world)) = joint_pose(input.world, input.joints.torso) else {
        return 0.0;
    };
    // The twist is expressed in the torso's own (world-aligned) frame, so in world terms it
    // pre-multiplies everything below the torso.
    let world_twist = torso_world
        .mul_quat(applied)
        .mul_quat(torso_world.inverse());
    let aimed = world_twist
        .mul_quat(hand_world)
        .mul_vec3(RIGHT_HAND_AIM_AXIS)
        .normalize_or_zero();
    aimed.angle_between(aim_dir.normalize_or_zero())
}

/// Fold the activity-driven reach & aim adjusters into `pose` in place (P31.15), and report
/// what they did.
///
/// Runs for every rigged avatar every frame: with no selection and no aim animation both
/// weights ease to zero and the fold is a no-op, so an avatar doing neither keeps its
/// animation pose untouched.
pub(crate) fn apply(
    pose: &mut AnimationPose,
    motion: &mut ReachMotion,
    input: &ReachInput<'_>,
) -> ReachReport {
    let state = motion.states.entry(input.agent).or_default();
    let dt = input.dt.max(0.0);

    // A **direction** in the avatar-local Second Life frame the deformed skeleton lives in:
    // both targets arrive as Bevy world points.
    let to_local_point = |world: Vec3| input.root.affine().inverse().transform_point3(world);

    // --- LLEditingMotion: reach the left hand toward the selection ---
    state.edit_weight = ease(
        state.edit_weight,
        input.point_at.is_some(),
        dt,
        0.0,
        EDITING_EASE_OUT,
    );
    let mut point_error = 0.0;
    if state.edit_weight > f32::EPSILON {
        // With the target gone, the arm keeps easing out of the *last* solved pose rather
        // than re-solving against nothing.
        if let Some(point) = input.point_at {
            point_error = apply_editing(pose, input, state, to_local_point(point));
        } else {
            let weight = state.edit_weight;
            let joints = input.joints;
            if let (Some(shoulder), Some(elbow)) = (state.last_shoulder, state.last_elbow) {
                blend_into(pose, joints.shoulder, shoulder, weight);
                blend_into(pose, joints.elbow, elbow, weight);
                blend_into(pose, joints.wrist, Quat::IDENTITY, weight);
                blend_into(pose, joints.torso, Quat::IDENTITY, weight);
            }
        }
    } else {
        state.last_shoulder = None;
        state.last_elbow = None;
    }

    // --- LLTargetingMotion: twist the torso until the right hand aims at the look-at ---
    let aim_dir = input.look_at.map(|point| {
        // From the avatar's own origin (the reference normalises the look-at *offset* from
        // the character's position), in the avatar-local frame.
        to_local_point(point)
    });
    state.aim_weight = ease(
        state.aim_weight,
        input.aiming && aim_dir.is_some(),
        dt,
        TARGETING_EASE_IN,
        TARGETING_EASE_OUT,
    );
    let mut torso_twist = 0.0;
    let mut aim_residual_angle = 0.0;
    if state.aim_weight > f32::EPSILON {
        let aim = if input.aiming { aim_dir } else { None };
        torso_twist = apply_targeting(pose, input, state, aim);
        if let Some(aim) = aim {
            let applied = Quat::IDENTITY.slerp(state.torso_delta, state.aim_weight.clamp(0.0, 1.0));
            aim_residual_angle = aim_residual(input, aim, applied);
        }
    } else {
        state.torso_delta = Quat::IDENTITY;
    }

    ReachReport {
        edit_weight: state.edit_weight,
        point_error,
        aim_weight: state.aim_weight,
        torso_twist,
        aim_residual: aim_residual_angle,
        aim_dir: aim_dir.unwrap_or(Vec3::ZERO).normalize_or_zero(),
    }
}

/// The live diagnostic (env `SL_VIEWER_LOG_REACH=1`): whether to log each avatar's reach /
/// aim state.
#[must_use]
pub(crate) fn log_enabled() -> bool {
    std::env::var("SL_VIEWER_LOG_REACH").as_deref() == Ok("1")
}

/// Select the object under the crosshair as the own avatar's editing target ([`SELECT_KEY`]),
/// or clear the selection when the key is pressed with nothing under it.
///
/// This is the viewer's stand-in for the reference's `LLSelectMgr` selection, which is what
/// feeds its point-at effect: it records the object *and* where on it the ray struck, in the
/// object's own frame, so the reach follows the object as it moves and the offset can be sent
/// on the wire verbatim.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the key, the \
              camera to cast from, the ray caster, the parent / object components the hit is \
              resolved through, and the selection it writes"
)]
pub(crate) fn select_object_under_crosshair(
    keyboard: Res<ButtonInput<KeyCode>>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    mut ray_cast: MeshRayCast,
    parents: Query<&ChildOf>,
    scene: Query<&SceneObject>,
    globals: Query<&GlobalTransform>,
    objects: Res<ObjectState>,
    mut selection: ResMut<PointAtSelection>,
) {
    if !keyboard.just_pressed(SELECT_KEY) {
        return;
    }
    // The *fly* camera, not any `Camera3d`: the reflection probes (P33) spawn their own
    // cameras, so a bare `With<Camera3d>` query has several matches and returns nothing.
    let Ok(camera) = camera.single() else {
        warn!("P31.15 select: no fly camera to cast from");
        return;
    };
    let ray = Ray3d::new(camera.translation(), camera.forward());
    let hits = ray_cast.cast_ray(ray, &MeshRayCastSettings::default());
    let Some((entity, hit)) = hits.first() else {
        info!("P31.15 select: nothing under the crosshair — selection cleared");
        selection.selected = None;
        return;
    };
    // The ray strikes a face entity: walk up the linkset to the nearest entity that *is* an
    // object, which is the one the point-at effect names.
    let mut current = *entity;
    let object = loop {
        if let Ok(object) = scene.get(current) {
            break Some((current, object.scoped_id));
        }
        let Ok(child_of) = parents.get(current) else {
            break None;
        };
        current = child_of.parent();
    };
    let (Some((entity, scoped)), Ok(global)) = (object, globals.get(current)) else {
        warn!("P31.15 select: hit an entity with no object identity");
        return;
    };
    let Some(key) = objects.full_key(&scoped) else {
        warn!("P31.15 select: object {scoped:?} has no full key yet");
        return;
    };
    // The hit point in the object's own Second Life frame: the object entity's global
    // carries the Second Life → Bevy basis change, so undoing it recovers the offset the
    // wire format wants.
    let offset = global.affine().inverse().transform_point3(hit.point);
    info!(
        "P31.15 select: object {key} offset=({:.2},{:.2},{:.2})",
        offset.x, offset.y, offset.z
    );
    selection.selected = Some(Selection {
        entity,
        key,
        offset,
    });
}

/// Resolve the own avatar's selection into its point-at target each frame, and advertise it
/// to the simulator as a `ViewerEffect` so other viewers start the reach on this avatar too
/// (the reference's `LLHUDEffectPointAt`).
///
/// The target is re-resolved from the selected object's *current* transform every frame, so
/// the reach follows an object that moves. A selection whose object has despawned is dropped.
pub(crate) fn drive_own_point_at(
    time: Res<Time>,
    identity: Res<SlIdentity>,
    globals: Query<&GlobalTransform>,
    mut selection: ResMut<PointAtSelection>,
    mut targets: ResMut<PointAtTargets>,
    mut writer: MessageWriter<SlCommand>,
) {
    let now = time.elapsed_secs();
    let Some(own) = identity.agent_id else {
        return;
    };
    // A selection whose object has despawned is dropped, so the reach relaxes rather than
    // holding onto a stale target.
    let live = selection
        .selected
        .and_then(|selected| Some((selected, globals.get(selected.entity).ok()?)));
    let Some((selected, global)) = live else {
        selection.selected = None;
        targets.clear(own);
        // Tell other viewers the gesture ended, once — the reference clears the effect's
        // target object along with its type.
        if selection.advertised.take().is_some() {
            send_point_at(&mut writer, own, None, Vec3::ZERO, PointAtType::Clear);
        }
        return;
    };
    targets.set(own, global.transform_point(selected.offset), now);

    // Re-send the effect on a change and periodically while the selection stands (the
    // simulator relays it to everyone in range; other viewers time it out otherwise).
    let due = match selection.advertised {
        Some((key, sent_at)) => key != selected.key || now - sent_at >= POINT_AT_RESEND_INTERVAL,
        None => true,
    };
    if due {
        send_point_at(
            &mut writer,
            own,
            Some(selected.key),
            selected.offset,
            PointAtType::Select,
        );
        selection.advertised = Some((selected.key, now));
    }
}

/// Send one point-at `ViewerEffect` for the own avatar. `offset` is in the target object's
/// own Second Life frame (the wire format's position field is an offset when a target object
/// is set, and a global position when it is not).
fn send_point_at(
    writer: &mut MessageWriter<SlCommand>,
    own: AgentKey,
    target: Option<ObjectKey>,
    offset: Vec3,
    point_at_type: PointAtType,
) {
    let effect = ViewerEffect {
        id: Uuid::new_v4(),
        agent_id: own,
        effect_type: ViewerEffectType::PointAt,
        duration: POINT_AT_EFFECT_DURATION,
        color: POINT_AT_EFFECT_COLOR,
        data: ViewerEffectData::PointAt {
            source: Some(own),
            target,
            target_position: GlobalCoordinates::new(
                f64::from(offset.x),
                f64::from(offset.y),
                f64::from(offset.z),
            ),
            point_at_type,
        },
    };
    writer.write(SlCommand(Command::ViewerEffect(vec![effect])));
}

/// Ingest other avatars' point-at effects into [`PointAtTargets`], so their editing reach
/// runs here too — the reference starts `ANIM_AGENT_EDITING` on the *source* avatar of every
/// point-at effect it receives. Also prunes expired targets.
///
/// The wire position is an **offset in the target object's frame** when the effect names one,
/// and a global position only when it does not; both are resolved into the scene here. An
/// effect naming an object this viewer has not seen is skipped rather than placed at the
/// region origin.
pub(crate) fn receive_point_at_effects(
    time: Res<Time>,
    identity: Res<SlIdentity>,
    mut events: MessageReader<SlEvent>,
    objects: Res<ObjectState>,
    globals: Query<&GlobalTransform>,
    mut targets: ResMut<PointAtTargets>,
) {
    let now = time.elapsed_secs();
    let own = identity.agent_id;
    let origin = identity.region_handle;
    for event in events.read() {
        let SlSessionEvent::ViewerEffect(effects) = &event.0 else {
            continue;
        };
        for effect in effects {
            let ViewerEffectData::PointAt {
                source,
                target,
                target_position,
                point_at_type,
            } = &effect.data
            else {
                continue;
            };
            let Some(source) = *source else {
                continue;
            };
            // The own avatar's gesture is driven from its own selection, so an echoed effect
            // is ignored.
            if own == Some(source) {
                continue;
            }
            if matches!(point_at_type, PointAtType::None | PointAtType::Clear) {
                targets.clear(source);
                continue;
            }
            let offset = Vec3::new(
                narrow(target_position.x()),
                narrow(target_position.y()),
                narrow(target_position.z()),
            );
            let point = match target {
                // An offset in the target object's own frame: its entity's global carries the
                // Second Life → Bevy basis change, so it maps the offset straight into the scene.
                Some(key) => objects
                    .entity_of(*key)
                    .and_then(|entity| globals.get(entity).ok())
                    .map(|global| global.transform_point(offset)),
                // No target object: the position is a global one.
                None => global_to_bevy(offset, origin),
            };
            let Some(point) = point else {
                continue;
            };
            targets.set(source, point, now);
        }
    }
    targets.prune(now);
}

/// Start / stop the own avatar's aim animation ([`AIM_KEY`]) through the simulator, which
/// echoes it back as an ordinary signalled animation — so the targeting motion switches on
/// through exactly the path a scripted weapon would drive, and other viewers see the avatar
/// aim too.
pub(crate) fn drive_aim_animation(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut aiming: Local<bool>,
    mut writer: MessageWriter<SlCommand>,
) {
    if !keyboard.just_pressed(AIM_KEY) {
        return;
    }
    let Some(builtin) = sl_anim::builtin_animation_by_name(AIM_ANIMATION) else {
        return;
    };
    *aiming = !*aiming;
    let anim = AnimationKey::from(builtin.id);
    writer.write(SlCommand(if *aiming {
        Command::PlayAnimation(anim)
    } else {
        Command::StopAnimation(anim)
    }));
    info!("P31.15 own aim animation ({AIM_ANIMATION}) -> {}", *aiming);
}

/// Convert a global point-at position into Bevy world space, using the agent's current
/// region south-west corner as the scene origin (the same conversion the look-at effects
/// use). Returns [`None`] until the region handle is known.
fn global_to_bevy(global: Vec3, origin: Option<RegionHandle>) -> Option<Vec3> {
    let origin = origin?;
    let (corner_x, corner_y) = origin.global_coordinates();
    let local = sl_client_bevy::Vector {
        x: global.x - metres_to_f32(corner_x),
        y: global.y - metres_to_f32(corner_y),
        z: global.z,
    };
    Some(sl_to_bevy_vec(&local))
}

/// Narrow a global-metre `f64` to the `f32` the scene works in — negligible at world scale
/// once the region origin is subtracted, and unavoidable at the coordinate boundary.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "f64 → f32 narrowing at the coordinate boundary; std has no lossless \
              From/TryFrom for it, and the value is a bounded world coordinate"
)]
const fn narrow(metres: f64) -> f32 {
    metres as f32
}

#[cfg(test)]
mod tests {
    use super::{
        AgentReach, EDITING_EASE_OUT, TARGETING_EASE_IN, TORSO_ROTATION_CONSTRAINT, ease,
        edit_goal, torso_aim_delta,
    };
    use crate::look_at::constrain;
    use bevy::prelude::*;

    /// Absolute-difference float check (the workspace forbids bare `==` on floats).
    fn near(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() <= eps, "{a} not within {eps} of {b}");
    }

    /// The rotation angle of a quaternion, radians in `[0, π]`.
    fn angle(q: Quat) -> f32 {
        Quat::IDENTITY.angle_between(q)
    }

    /// A target in front of the avatar (on the near side of the edit plane) is reached for
    /// exactly where it is: the goal is the target, untouched.
    #[test]
    fn a_target_in_front_is_left_alone() {
        let shoulder = Vec3::new(0.0, 0.15, 1.3);
        // Second Life `+X` is forward: 2 m ahead and a little left.
        let target = Vec3::new(2.0, 0.4, 1.2);
        let goal = edit_goal(shoulder, Quat::IDENTITY, target);
        assert!(
            goal.abs_diff_eq(target, 1e-4),
            "goal {goal:?} should be the target {target:?}",
        );
    }

    /// A target *behind* the avatar is lifted steeply upward, so the arm reaches up and over
    /// rather than through the chest — and the distance to the target is preserved either way.
    #[test]
    fn a_target_behind_is_folded_upward() {
        let shoulder = Vec3::new(0.0, 0.15, 1.3);
        // Straight behind the avatar (Second Life −X), level with the shoulder.
        let behind = Vec3::new(-2.0, 0.15, 1.3);
        let goal = edit_goal(shoulder, Quat::IDENTITY, behind);
        let to_goal = Vec3::new(
            goal.x - shoulder.x,
            goal.y - shoulder.y,
            goal.z - shoulder.z,
        );
        // The reach keeps the target's distance…
        near(to_goal.length(), 2.0, 1e-4);
        // …and now points mostly *up* rather than back, which is the whole point: an arm
        // cannot reach through the body, so the reference sends it overhead instead.
        assert!(
            to_goal.z > to_goal.x.abs(),
            "goal should be lifted above the backwards reach: {to_goal:?}"
        );
    }

    /// The edit plane rides with the torso: the same world target, with the avatar's torso
    /// turned to face it, is *in front* of the plane and so is not folded at all.
    #[test]
    fn the_edit_plane_turns_with_the_torso() {
        let shoulder = Vec3::ZERO;
        let target = Vec3::new(-2.0, 0.0, 0.0);
        // Facing forward, the target is behind: it gets folded (and lifted off the ground).
        let folded = edit_goal(shoulder, Quat::IDENTITY, target);
        assert!(folded.z > 0.5, "a target behind should be folded upward");
        // Turned 180° about up, the same target is now in front: untouched.
        let turned = edit_goal(
            shoulder,
            Quat::from_rotation_z(core::f32::consts::PI),
            target,
        );
        assert!(
            turned.abs_diff_eq(target, 1e-4),
            "a target the torso faces should be left alone, got {turned:?}",
        );
    }

    /// A right hand whose aim axis (its own −Y) points along Second Life forward (+X) — the
    /// fixture the targeting tests aim from.
    fn forward_aiming_hand() -> Quat {
        let hand = Quat::from_rotation_z(core::f32::consts::FRAC_PI_2);
        let aim = hand.mul_vec3(Vec3::new(0.0, -1.0, 0.0));
        assert!(aim.abs_diff_eq(Vec3::X, 1e-5), "hand aims {aim:?}");
        hand
    }

    /// The targeting twist is the rotation that carries the right hand's aim onto the target:
    /// composing it onto the torso's world rotation turns the hand's aim axis onto the target
    /// direction.
    #[test]
    fn the_torso_twist_turns_the_hand_onto_the_target() {
        let hand = forward_aiming_hand();
        // A target 45° to the avatar's left.
        let target = Vec3::new(1.0, 1.0, 0.0).normalize();
        let torso = Quat::IDENTITY;
        let delta = torso_aim_delta(target, torso, hand);
        // The twist, applied in the torso's frame, carries the hand's aim onto the target.
        let aimed = torso
            .mul_quat(delta)
            .mul_quat(torso.inverse())
            .mul_quat(hand);
        let new_at = aimed.mul_vec3(Vec3::new(0.0, -1.0, 0.0));
        near(new_at.dot(target), 1.0, 1e-4);
        // A 45° turn, well inside the torso limit.
        near(angle(delta), core::f32::consts::FRAC_PI_4, 1e-4);
    }

    /// A hand already on target needs no twist at all.
    #[test]
    fn an_aimed_hand_needs_no_twist() {
        let delta = torso_aim_delta(Vec3::X, Quat::IDENTITY, forward_aiming_hand());
        near(angle(delta), 0.0, 1e-5);
    }

    /// A target straight overhead degenerates the aim basis; the motion yields no twist
    /// rather than NaNs.
    #[test]
    fn a_vertical_target_yields_no_twist() {
        let delta = torso_aim_delta(Vec3::Z, Quat::IDENTITY, Quat::IDENTITY);
        near(angle(delta), 0.0, 1e-6);
        assert!(delta.is_finite());
    }

    /// However far the ideal twist wants to turn the torso, the *total* rotation (keyframe
    /// plus twist) stays inside the reference's limit.
    #[test]
    fn the_total_torso_rotation_is_constrained() {
        // A target directly behind the avatar wants a ~180° twist.
        let delta = torso_aim_delta(
            Vec3::new(-1.0, 0.0, 0.0),
            Quat::IDENTITY,
            forward_aiming_hand(),
        );
        assert!(angle(delta) > TORSO_ROTATION_CONSTRAINT);
        // The applied twist is the constrained one (the fold constrains keyframe · delta).
        let applied = constrain(Quat::IDENTITY.mul_quat(delta), TORSO_ROTATION_CONSTRAINT);
        assert!(angle(applied) <= TORSO_ROTATION_CONSTRAINT + 1e-4);
    }

    /// The editing reach engages on the frame its target appears (the reference's zero
    /// ease-in) and fades out over its ease-out duration; the targeting twist eases in over
    /// its own.
    #[test]
    fn weights_ease_the_way_the_reference_does() {
        let dt = 1.0 / 60.0;
        // Editing: full weight immediately.
        near(ease(0.0, true, dt, 0.0, EDITING_EASE_OUT), 1.0, 1e-6);
        // …and back to zero over the ease-out (not instantly).
        let mut weight = 1.0;
        let mut steps = 0;
        while weight > 0.0 {
            weight = ease(weight, false, dt, 0.0, EDITING_EASE_OUT);
            steps += 1;
            assert!(steps < 1000, "the editing reach never eased out");
        }
        assert!(steps > 20, "the editing reach eased out in {steps} frames");
        // Targeting: a gradual ease-in.
        let stepped = ease(0.0, true, dt, TARGETING_EASE_IN, 0.0);
        assert!(
            stepped > 0.0 && stepped < 0.2,
            "targeting eased in too fast: {stepped}"
        );
    }

    /// A fresh avatar starts with no reach and no aim, so the fold is a no-op until something
    /// engages it.
    #[test]
    fn a_fresh_avatar_reaches_for_nothing() {
        let state = AgentReach::default();
        near(state.edit_weight, 0.0, 0.0);
        near(state.aim_weight, 0.0, 0.0);
        assert!(state.last_shoulder.is_none());
    }
}
