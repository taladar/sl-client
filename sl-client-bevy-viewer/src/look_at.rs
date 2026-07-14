//! Head & eye look-at tracking (P31.12): the reference viewer's `LLHeadRotMotion`
//! and `LLEyeMotion` procedural adjusters, ported to run on top of every rigged
//! avatar's sampled keyframe pose (P18.3) — the same pose-apply seam the P31.8
//! idle adjusters ([`crate::procedural`]) use.
//!
//! Both motions turn the avatar toward a **world look-at target**. Two sources
//! provide it:
//!
//! - **Own avatar**: the debug fly-camera's own position, so the avatar tracks
//!   (makes eye contact with) the viewer's camera — the clearest stand-in for the
//!   reference's mouselook / cursor-focus point, which the free-fly camera lacks.
//! - **Other avatars**: the sim-relayed `ViewerEffect` **look-at** effect (the
//!   P11-era [`ViewerEffectData::LookAt`](sl_client_bevy::ViewerEffectData) the
//!   viewer already surfaces as [`SlSessionEvent::ViewerEffect`]), whose global
//!   target position is converted into the scene.
//!
//! What is ported here (the two joint-rotation motions):
//!
//! - **`LLHeadRotMotion`** turns the head toward the target and lags the neck
//!   ([`NECK_LAG`]) behind it, constrained to [`HEAD_ROTATION_CONSTRAINT`]; with no
//!   target it faces forward (rest), so it is a near no-op until a target exists.
//!   (The reference also lags the torso a little; that is dropped here — see below.)
//! - **`LLEyeMotion`** aims the eyes at the target with vergence and layers
//!   random saccade / look-away jitter on top (bounded by [`EYE_ROT_LIMIT_ANGLE`]),
//!   and drives the eye **blink** (P31.12b): a random blink timer
//!   ([`EYE_BLINK_MIN_TIME`]..[`EYE_BLINK_MAX_TIME`] between blinks) morphs the
//!   `Blink_Left` / `Blink_Right` visual-params shut and open each blink, plus an
//!   opportunistic blink whenever the eyes look away. The eyelid morphs are driven
//!   every frame through the per-frame runtime-morph pipeline ([`crate::avatars`]
//!   `AvatarRuntimeMorphs`, P31.12a) rather than the joint pose, since the
//!   appearance pipeline bakes shape morphs into geometry once at appearance time
//!   and cannot animate a param per frame.
//!
//! Like the idle adjusters this is viewer-only (no runtime parity). Unlike them the
//! aim **replaces** the neck / head keyframe while engaged (the reference's
//! head-track is a high-priority motion, not an additive one) — folding it as a
//! delta on top of the keyframe lets it drift with the animation's own head motion
//! each loop, so the gaze would not hold. Crucially each driven joint's *local*
//! rotation is derived from its parent's **actual current world rotation** (the
//! animation + idle pose is already folded into the deformed skeleton this reads),
//! so the head lands on the target regardless of what the animation does to the
//! spine — an assumed rest spine sent the head somewhere unrelated. The torso lag
//! is dropped because driving the torso would invalidate that parent-world read for
//! the neck (the intermediate `mChest` is left to the animation). An eased
//! per-avatar weight blends between the keyframe (idle) and the absolute aim so a
//! gaze engages and relaxes smoothly. The math is pure and unit-tested; the
//! per-avatar smoothing / saccade / weight state lives in [`LookAtMotion`], and the
//! world targets in [`LookAtTargets`].

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, AnimationPose, LookAtType, RegionHandle, SlEvent, SlIdentity, SlSessionEvent,
    ViewerEffectData,
};

use crate::camera::FlyCamera;
use crate::coords::{metres_to_f32, sl_to_bevy_vec};

/// Neck rotation factor — the fraction of the head's look-at rotation the neck
/// takes (in world space), the head completing the rest (reference `NECK_LAG`).
const NECK_LAG: f32 = 0.5;

/// Half-life (seconds) of the head's slerp toward its look-at target (reference
/// `HEAD_LOOKAT_LAG_HALF_LIFE`).
const HEAD_LOOKAT_LAG_HALF_LIFE: f32 = 0.15;

/// Half-life (seconds) of the "aiming" weight that blends head / neck
/// between the animation keyframe and the absolute look-at aim — how fast the
/// avatar engages a new gaze and relaxes back to its idle head motion when the
/// gaze ends. Not a reference constant (the reference blends by motion priority);
/// tuned to ease in / out over a few tenths of a second.
const LOOK_AT_WEIGHT_HALF_LIFE: f32 = 0.2;

/// Limit angle (radians) for the head's local rotation (reference
/// `HEAD_ROTATION_CONSTRAINT`, `π/2 · 0.8`).
const HEAD_ROTATION_CONSTRAINT: f32 = core::f32::consts::FRAC_PI_2 * 0.8;

/// Minimum head-to-target distance (metres) before the head turns to look at it;
/// closer than this the head stays at rest (reference `MIN_HEAD_LOOKAT_DISTANCE`).
const MIN_HEAD_LOOKAT_DISTANCE: f32 = 0.3;

/// Squared magnitude below which the up × look-at cross product is treated as
/// degenerate (look-at nearly parallel to the up axis), reference `0.15f`.
const LOOK_AT_LEFT_DEGENERATE: f32 = 0.15;

/// Blend fraction used to pull a near-vertical look-at back toward the root
/// forward so the head basis stays well-conditioned (reference `lerp(..., 0.4f)`).
const LOOK_AT_FORWARD_PULL: f32 = 0.4;

/// Min seconds between eye jitter (saccade) motions (reference `EYE_JITTER_MIN_TIME`).
const EYE_JITTER_MIN_TIME: f32 = 0.3;

/// Max seconds between eye jitter (saccade) motions (reference `EYE_JITTER_MAX_TIME`).
const EYE_JITTER_MAX_TIME: f32 = 2.5;

/// Max yaw (radians) of an eye jitter motion (reference `EYE_JITTER_MAX_YAW`).
const EYE_JITTER_MAX_YAW: f32 = 0.08;

/// Max pitch (radians) of an eye jitter motion (reference `EYE_JITTER_MAX_PITCH`).
const EYE_JITTER_MAX_PITCH: f32 = 0.015;

/// Min seconds between eye look-away motions (reference `EYE_LOOK_AWAY_MIN_TIME`).
const EYE_LOOK_AWAY_MIN_TIME: f32 = 5.0;

/// Max seconds between eye look-away motions (reference `EYE_LOOK_AWAY_MAX_TIME`).
const EYE_LOOK_AWAY_MAX_TIME: f32 = 15.0;

/// Min seconds before looking back after looking away (reference
/// `EYE_LOOK_BACK_MIN_TIME`).
const EYE_LOOK_BACK_MIN_TIME: f32 = 1.0;

/// Max seconds before looking back after looking away (reference
/// `EYE_LOOK_BACK_MAX_TIME`).
const EYE_LOOK_BACK_MAX_TIME: f32 = 5.0;

/// Max yaw (radians) of an eye look-away motion (reference `EYE_LOOK_AWAY_MAX_YAW`).
const EYE_LOOK_AWAY_MAX_YAW: f32 = 0.15;

/// Max pitch (radians) of an eye look-away motion (reference `EYE_LOOK_AWAY_MAX_PITCH`).
const EYE_LOOK_AWAY_MAX_PITCH: f32 = 0.12;

/// Limit angle (radians) for eye rotation, keeping the gaze in front of the face
/// (reference `EYE_ROT_LIMIT_ANGLE`, `π/2 · 0.3`).
const EYE_ROT_LIMIT_ANGLE: f32 = core::f32::consts::FRAC_PI_2 * 0.3;

/// Foveal angular offset (radians) added to the vergence so the eyes converge on
/// the fovea rather than the pupil (reference `4° · DEG_TO_RAD`).
const FOVEAL_OFFSET: f32 = 4.0 * (core::f32::consts::PI / 180.0);

/// Vergence threshold above which the eyes still jitter; below it (a very near
/// target, eyes strongly crossed) jitter is suppressed (reference `-0.05f`).
const VERGENCE_JITTER_THRESHOLD: f32 = -0.05;

/// Minimum seconds between eye blinks (reference `EYE_BLINK_MIN_TIME`).
const EYE_BLINK_MIN_TIME: f32 = 0.5;

/// Maximum seconds between eye blinks (reference `EYE_BLINK_MAX_TIME`).
const EYE_BLINK_MAX_TIME: f32 = 8.0;

/// Seconds an eye stays fully shut in the middle of a blink (reference
/// `EYE_BLINK_CLOSE_TIME`).
const EYE_BLINK_CLOSE_TIME: f32 = 0.03;

/// Seconds one eyelid takes to fully open or close during a blink — the morph
/// ramp rate (reference `EYE_BLINK_SPEED`).
const EYE_BLINK_SPEED: f32 = 0.015;

/// Seconds the right eye lags the left in a blink, so the two eyelids do not move
/// in perfect lockstep (reference `EYE_BLINK_TIME_DELTA`).
const EYE_BLINK_TIME_DELTA: f32 = 0.005;

/// Probability (per look-away toggle) that the eyes do **not** take the chance to
/// blink while moving — the reference blinks when `ll_frand() > 0.1`, i.e. an
/// opportunistic blink fires on 90% of look-away toggles (reference
/// `LLEyeMotion::onUpdate`).
const EYE_BLINK_SKIP_WHILE_MOVING: f32 = 0.1;

/// The `avatar_lad.xml` visual-param name of the left eyelid blink morph the blink
/// timer drives through the per-frame runtime-morph pipeline (P31.12a); reference
/// `LLEyeMotion::onUpdate`'s `setVisualParamWeight("Blink_Left", …)`.
pub(crate) const BLINK_LEFT_PARAM: &str = "Blink_Left";

/// The `avatar_lad.xml` visual-param name of the right eyelid blink morph (see
/// [`BLINK_LEFT_PARAM`]).
pub(crate) const BLINK_RIGHT_PARAM: &str = "Blink_Right";

/// Seconds a received `ViewerEffect` look-at target stays valid before it is
/// pruned and the head returns to rest. The reference resends an active look-at
/// periodically; this outlives the resend interval but drops a stale gaze.
const LOOK_AT_TARGET_TTL: f32 = 3.0;

/// A world look-at target for one avatar: a point in Bevy world space and the
/// time (seconds since startup) it expires. The own avatar's target is refreshed
/// every frame from the camera, so it never expires in practice.
struct LookAtTarget {
    /// The look-at point in Bevy world space.
    point: Vec3,
    /// Elapsed-seconds timestamp after which the target is pruned.
    expires_at: f32,
}

/// The current world look-at target of each avatar that has one, keyed by agent.
/// Populated by [`update_own_look_at_target`] (own avatar, from the fly-camera)
/// and [`receive_look_at_effects`] (others, from `ViewerEffect`); consumed by the
/// pose pass to aim the head and eyes.
#[derive(Resource, Default)]
pub(crate) struct LookAtTargets {
    /// Per-avatar look-at points, keyed by agent id.
    targets: HashMap<AgentKey, LookAtTarget>,
}

impl LookAtTargets {
    /// The Bevy-world look-at point for `agent`, if it currently has one.
    pub(crate) fn point(&self, agent: AgentKey) -> Option<Vec3> {
        self.targets.get(&agent).map(|target| target.point)
    }

    /// Record `point` (Bevy world space) as `agent`'s look-at target, valid until
    /// `now + LOOK_AT_TARGET_TTL`.
    fn set(&mut self, agent: AgentKey, point: Vec3, now: f32) {
        self.targets.insert(
            agent,
            LookAtTarget {
                point,
                expires_at: now + LOOK_AT_TARGET_TTL,
            },
        );
    }

    /// Drop `agent`'s look-at target (a cleared / ended gaze).
    fn clear(&mut self, agent: AgentKey) {
        self.targets.remove(&agent);
    }

    /// Prune every target whose expiry has passed at `now`.
    fn prune(&mut self, now: f32) {
        self.targets
            .retain(|_agent, target| target.expires_at > now);
    }
}

/// A tiny deterministic SplitMix64 PRNG, seeded per avatar so each avatar's eye
/// saccades are decorrelated yet reproducible (no global RNG, and the reference's
/// `ll_frand` table is itself re-seeded every startup, so there is no canonical
/// waveform to match — only the character of the motion).
struct Rng {
    /// The 64-bit generator state.
    state: u64,
}

impl Rng {
    /// A generator seeded from `seed`.
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next 64-bit output (SplitMix64), advancing the state.
    const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform `f32` in `[0, 1)` with 16-bit resolution — enough for jitter, and
    /// cast-free (`f32::from(u16)` rather than the forbidden `as`).
    fn unit(&mut self) -> f32 {
        let bits = self.next_u64() >> 48;
        let narrowed = u16::try_from(bits & 0xFFFF).unwrap_or(0);
        f32::from(narrowed) / 65_536.0
    }

    /// A uniform `f32` in `[-1, 1)`.
    fn signed(&mut self) -> f32 {
        self.unit().mul_add(2.0, -1.0)
    }
}

/// The eye saccade + blink state machine (reference `LLEyeMotion`'s jitter /
/// look-away / blink timers), advanced each frame to produce a small extra yaw /
/// pitch layered on the eye aim and the two eyelid morph weights (P31.12b).
struct Saccade {
    /// Seconds elapsed on the jitter timer since it last fired.
    jitter_elapsed: f32,
    /// The randomly-chosen interval until the next jitter.
    jitter_time: f32,
    /// Current jitter yaw (radians).
    jitter_yaw: f32,
    /// Current jitter pitch (radians).
    jitter_pitch: f32,
    /// The threshold (on the jitter timer) at which the look-away state toggles.
    look_away_time: f32,
    /// Current look-away yaw (radians); zero when not looking away.
    look_away_yaw: f32,
    /// Current look-away pitch (radians); zero when not looking away.
    look_away_pitch: f32,
    /// Seconds elapsed on the blink timer since it last reset (reference
    /// `mEyeBlinkTimer`).
    blink_elapsed: f32,
    /// The blink timer threshold: while the eyes are open, the wait until the next
    /// blink starts; while they are shut, the [`EYE_BLINK_CLOSE_TIME`] hold before
    /// they reopen (reference `mEyeBlinkTime`).
    blink_time: f32,
    /// Whether the eyes are currently in the shut-then-opening half of a blink
    /// (reference `mEyesClosed`).
    eyes_closed: bool,
}

impl Default for Saccade {
    /// Start with the timers expired so the first frame picks fresh values.
    fn default() -> Self {
        Self {
            jitter_elapsed: 0.0,
            jitter_time: 0.0,
            jitter_yaw: 0.0,
            jitter_pitch: 0.0,
            look_away_time: EYE_LOOK_AWAY_MIN_TIME,
            look_away_yaw: 0.0,
            look_away_pitch: 0.0,
            blink_elapsed: 0.0,
            blink_time: 0.0,
            eyes_closed: false,
        }
    }
}

/// The combined saccade offset applied to the eyes this frame.
#[derive(Clone, Copy)]
struct SaccadeOffset {
    /// Total yaw (radians): jitter + look-away.
    yaw: f32,
    /// Total pitch (radians): jitter + look-away.
    pitch: f32,
}

/// The eye-blink morph weights for one frame: how shut each eyelid is, `0.0`
/// (open) .. `1.0` (fully closed), driving the `Blink_Left` / `Blink_Right`
/// visual-param morphs through the per-frame runtime-morph pipeline (P31.12a).
#[derive(Clone, Copy, Default)]
pub(crate) struct BlinkWeights {
    /// The left eyelid morph weight ([`BLINK_LEFT_PARAM`]).
    pub(crate) left: f32,
    /// The right eyelid morph weight ([`BLINK_RIGHT_PARAM`]).
    pub(crate) right: f32,
}

/// The eye state produced by one [`Saccade::advance`]: the jitter / look-away
/// rotation offset for the eye aim plus this frame's blink morph weights.
#[derive(Clone, Copy)]
struct EyeSaccade {
    /// The jitter + look-away rotation offset.
    offset: SaccadeOffset,
    /// The eyelid blink morph weights.
    blink: BlinkWeights,
}

impl Saccade {
    /// Advance the jitter / look-away / blink timers by `dt` seconds, drawing new
    /// random targets as they fire, and return the eye offset + blink weights for
    /// this frame. Mirrors `LLEyeMotion::onUpdate`'s timer logic.
    fn advance(&mut self, dt: f32, rng: &mut Rng) -> EyeSaccade {
        let dt = dt.max(0.0);
        self.jitter_elapsed += dt;
        self.blink_elapsed += dt;
        if self.jitter_elapsed > self.jitter_time {
            self.jitter_time =
                EYE_JITTER_MIN_TIME + rng.unit() * (EYE_JITTER_MAX_TIME - EYE_JITTER_MIN_TIME);
            self.jitter_yaw = rng.signed() * EYE_JITTER_MAX_YAW;
            self.jitter_pitch = rng.signed() * EYE_JITTER_MAX_PITCH;
            // The look-away timer shares the jitter clock in the reference, so
            // carry its countdown across this reset.
            self.look_away_time -= self.jitter_elapsed.max(0.0);
            self.jitter_elapsed = 0.0;
        } else if self.jitter_elapsed > self.look_away_time {
            // Blink while moving the eyes some percentage of the time (reference:
            // `ll_frand() > 0.1f`): force the blink threshold to now so a blink
            // starts this frame. Drawn before the look-away offsets to keep the
            // reference's RNG-draw order.
            if rng.unit() > EYE_BLINK_SKIP_WHILE_MOVING {
                self.blink_time = self.blink_elapsed;
            }
            if self.look_away_yaw == 0.0 && self.look_away_pitch == 0.0 {
                // Start a look-away: pick an off-target offset held for a short
                // "look back" interval.
                self.look_away_yaw = rng.signed() * EYE_LOOK_AWAY_MAX_YAW;
                self.look_away_pitch = rng.signed() * EYE_LOOK_AWAY_MAX_PITCH;
                self.look_away_time = EYE_LOOK_BACK_MIN_TIME
                    + rng.unit() * (EYE_LOOK_BACK_MAX_TIME - EYE_LOOK_BACK_MIN_TIME);
            } else {
                // End the look-away: return to the target and wait a longer interval.
                self.look_away_yaw = 0.0;
                self.look_away_pitch = 0.0;
                self.look_away_time = EYE_LOOK_AWAY_MIN_TIME
                    + rng.unit() * (EYE_LOOK_AWAY_MAX_TIME - EYE_LOOK_AWAY_MIN_TIME);
            }
        }
        EyeSaccade {
            offset: SaccadeOffset {
                yaw: self.jitter_yaw + self.look_away_yaw,
                pitch: self.jitter_pitch + self.look_away_pitch,
            },
            blink: self.advance_blink(rng),
        }
    }

    /// Advance the blink half of the state machine and return this frame's eyelid
    /// morph weights, mirroring the "do blinking" block of `LLEyeMotion::onUpdate`.
    ///
    /// Uses the blink timer already advanced by [`advance`](Self::advance) this
    /// frame. The right eye lags the left by [`EYE_BLINK_TIME_DELTA`], each eyelid
    /// ramping shut / open over [`EYE_BLINK_SPEED`]; a completed close holds the
    /// eyes shut for [`EYE_BLINK_CLOSE_TIME`], and a completed open picks the next
    /// random blink interval. Between blinks the eyes stay fully open (`0.0`).
    fn advance_blink(&mut self, rng: &mut Rng) -> BlinkWeights {
        if !self.eyes_closed {
            if self.blink_elapsed < self.blink_time {
                // Waiting between blinks: eyes fully open.
                return BlinkWeights::default();
            }
            // Closing: both eyelids ramp 0 → 1, the right lagging the left.
            let since = self.blink_elapsed - self.blink_time;
            let left = (since / EYE_BLINK_SPEED).clamp(0.0, 1.0);
            let right = ((since - EYE_BLINK_TIME_DELTA) / EYE_BLINK_SPEED).clamp(0.0, 1.0);
            if right >= 1.0 {
                // Fully shut: hold closed for the close time before reopening.
                self.eyes_closed = true;
                self.blink_time = EYE_BLINK_CLOSE_TIME;
                self.blink_elapsed = 0.0;
            }
            BlinkWeights { left, right }
        } else if self.blink_elapsed < self.blink_time {
            // Held shut for the close time: eyes fully closed.
            BlinkWeights {
                left: 1.0,
                right: 1.0,
            }
        } else {
            // Opening: both eyelids ramp 1 → 0, the right lagging the left.
            let since = self.blink_elapsed - self.blink_time;
            let left = 1.0 - (since / EYE_BLINK_SPEED).clamp(0.0, 1.0);
            let right = 1.0 - ((since - EYE_BLINK_TIME_DELTA) / EYE_BLINK_SPEED).clamp(0.0, 1.0);
            if right <= 0.0 {
                // Fully open: schedule the next blink.
                self.eyes_closed = false;
                self.blink_time =
                    EYE_BLINK_MIN_TIME + rng.unit() * (EYE_BLINK_MAX_TIME - EYE_BLINK_MIN_TIME);
                self.blink_elapsed = 0.0;
            }
            BlinkWeights { left, right }
        }
    }
}

/// The per-avatar look-at motion state: the head smoothing accumulator (reference
/// `mLastHeadRot`) and the eased aiming weight, plus the eye saccade
/// machine and its PRNG.
struct AgentLookAt {
    /// The smoothed head **world** aim rotation (avatar-local frame) from the
    /// previous frame — the orientation the head is easing toward its target.
    last_head_rot: Quat,
    /// The eased "aiming" weight in `[0, 1]`: how much the head / neck are driven by
    /// the look-at aim (1) versus their animation keyframe (0).
    weight: f32,
    /// The eye saccade timers.
    saccade: Saccade,
    /// This avatar's PRNG.
    rng: Rng,
}

impl AgentLookAt {
    /// Fresh state, with the PRNG seeded from `agent` so avatars are decorrelated.
    fn new(agent: AgentKey) -> Self {
        let (high, low) = agent.uuid().as_u64_pair();
        Self {
            last_head_rot: Quat::IDENTITY,
            weight: 0.0,
            saccade: Saccade::default(),
            rng: Rng::new(high ^ low),
        }
    }
}

/// Per-avatar [`AgentLookAt`] state, keyed by agent. Retained across frames so the
/// head smoothing, aiming weight and eye saccades are continuous.
#[derive(Resource, Default)]
pub(crate) struct LookAtMotion {
    /// The look-at motion state of each rigged avatar seen so far.
    states: HashMap<AgentKey, AgentLookAt>,
}

impl LookAtMotion {
    /// The mutable state for `agent`, created (seeded from the agent id) on first use.
    fn state_mut(&mut self, agent: AgentKey) -> &mut AgentLookAt {
        self.states
            .entry(agent)
            .or_insert_with(|| AgentLookAt::new(agent))
    }
}

/// The skeleton indices of the joints the look-at motions drive, resolved once per
/// avatar per frame. A joint the skeleton lacks is [`None`] and skipped (the alt
/// eye joints are absent on some skeletons).
#[derive(Clone, Copy, Default)]
pub(crate) struct LookAtJoints {
    /// `mNeck` index.
    pub(crate) neck: Option<usize>,
    /// `mHead` index.
    pub(crate) head: Option<usize>,
    /// `mEyeLeft` index.
    pub(crate) eye_left: Option<usize>,
    /// `mEyeRight` index.
    pub(crate) eye_right: Option<usize>,
    /// `mFaceEyeAltLeft` index (the "alt" eyeball some skeletons carry).
    pub(crate) alt_eye_left: Option<usize>,
    /// `mFaceEyeAltRight` index.
    pub(crate) alt_eye_right: Option<usize>,
}

/// Component-wise vector subtraction (`a - b`), avoiding the glam `-` operator the
/// workspace `arithmetic_side_effects` lint trips on.
fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise linear interpolation of vectors, `a + (b - a) * t`.
fn vlerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    Vec3::new(
        a.x + (b.x - a.x) * t,
        a.y + (b.y - a.y) * t,
        a.z + (b.z - a.z) * t,
    )
}

/// The per-frame slerp fraction for a given `half_life` (seconds) and frame time
/// `dt`, i.e. the reference `LLSmoothInterpolation::getInterpolant`
/// (`1 - 2^(-dt / half_life)`), clamped to `[0, 1]`. A non-positive half-life
/// snaps immediately.
fn smooth_interpolant(half_life: f32, dt: f32) -> f32 {
    if half_life <= 0.0 || !dt.is_finite() {
        return 1.0;
    }
    let frac = 1.0 - 2.0_f32.powf(-dt.max(0.0) / half_life);
    frac.clamp(0.0, 1.0)
}

/// Constrain a rotation to at most `max_angle` radians about its axis (the
/// reference `LLQuaternion::constrain`): rotations within the limit pass through,
/// larger ones are clamped to the limit keeping their axis.
fn constrain(rotation: Quat, max_angle: f32) -> Quat {
    let angle = Quat::IDENTITY.angle_between(rotation);
    if angle <= max_angle {
        return rotation;
    }
    let (axis, _angle) = rotation.to_axis_angle();
    Quat::from_axis_angle(axis, max_angle)
}

/// A rotation whose local `+X` (Second Life forward) points along `forward` with
/// `+Z` roughly up, built from the orthonormal basis `(forward, left, up)` the
/// reference `LLQuaternion(fwd, left, up)` constructor forms (`left = up × fwd`,
/// `up = fwd × left`). `forward` must be non-degenerate; a near-zero or
/// near-vertical `forward` is handled by the callers.
fn basis_rotation(forward: Vec3) -> Quat {
    let fwd = forward.normalize_or_zero();
    let left = Vec3::Z.cross(fwd).normalize_or_zero();
    let up = fwd.cross(left);
    Quat::from_mat3(&Mat3::from_cols(fwd, left, up))
}

/// The head's **unconstrained** target world rotation (avatar-local Second Life
/// frame, `+X` forward, `+Z` up) that faces `look_dir` — the reference
/// `LLHeadRotMotion::onUpdate` target orientation before the rotation limit. A
/// too-close or absent target returns identity (face rest forward). The limit is
/// applied later, relative to the **animated** upper body rather than this rest
/// frame (see [`apply_to_pose`]), so an idle animation that turns the body does not
/// make the symmetric limit look lopsided.
fn head_target_rotation(look_dir: Vec3) -> Quat {
    let distance = look_dir.length();
    if distance < MIN_HEAD_LOOKAT_DISTANCE {
        return Quat::IDENTITY;
    }
    let mut forward = look_dir.normalize_or_zero();
    // If the look-at is nearly parallel to the up axis the (up × look) basis
    // degenerates; pull it back toward the root forward (Second Life `+X`).
    if Vec3::Z.cross(forward).length_squared() < LOOK_AT_LEFT_DEGENERATE {
        forward = vlerp(forward, Vec3::X, LOOK_AT_FORWARD_PULL).normalize_or_zero();
    }
    basis_rotation(forward)
}

/// The eye aim rotation in head-local space for a look-at direction in the
/// avatar-local Second Life frame, given the head's accumulated world rotation
/// `head_world` (also avatar-local). Mirrors `LLEyeMotion::adjustEyeTarget`'s
/// target computation: aim, convert to head-local, strip roll, constrain.
fn eye_target_rotation(look_dir: Vec3, head_world: Quat) -> Quat {
    let world_aim = basis_rotation(look_dir);
    let head_local = head_world.inverse().mul_quat(world_aim);
    // Eliminate roll (the reference keeps only pitch / yaw), then constrain to keep
    // the gaze in front of the face.
    let (_roll, pitch, yaw) = head_local.to_euler(EulerRot::XYZ);
    let no_roll = Quat::from_euler(EulerRot::XYZ, 0.0, pitch, yaw);
    constrain(no_roll, EYE_ROT_LIMIT_ANGLE)
}

/// The eye vergence angle (radians) for a target `distance` metres away and eyes
/// `interocular` metres apart, with the foveal offset applied (reference
/// `adjustEyeTarget`). Clamped so the eyes never diverge or cross past a right
/// angle.
fn eye_vergence(interocular: f32, distance: f32) -> f32 {
    let raw = -(interocular / 2.0).atan2(distance.max(f32::EPSILON));
    raw.clamp(-core::f32::consts::FRAC_PI_2, 0.0) + FOVEAL_OFFSET
}

/// Fold `delta` onto joint `index`'s current pose rotation (`base · delta`), the
/// same additive convention the idle adjusters use, so a playing animation still
/// dominates the joint.
fn compose(pose: &mut AnimationPose, index: Option<usize>, delta: Quat) {
    if let Some(index) = index {
        let base = pose.rotation(index).unwrap_or(Quat::IDENTITY);
        pose.set_rotation(index, base.mul_quat(delta));
    }
}

/// Blend joint `index`'s pose rotation from its current (animation keyframe)
/// rotation toward the absolute look-at local rotation `aim` by `weight` in
/// `[0, 1]` — `weight = 0` leaves the keyframe untouched, `weight = 1` replaces it
/// with the aim. Unlike [`compose`], the aim **replaces** rather than layers, so a
/// held gaze does not drift with the animation's own head motion.
fn set_blended(pose: &mut AnimationPose, index: Option<usize>, aim: Quat, weight: f32) {
    if let Some(index) = index {
        let keyframe = pose.rotation(index).unwrap_or(Quat::IDENTITY);
        pose.set_rotation(index, keyframe.slerp(aim, weight.clamp(0.0, 1.0)));
    }
}

/// The eye pair rotations `(left, right)` for a head-local aim `target`, a
/// `vergence` angle, whether there is an actual target (`aiming`), and the frame's
/// `saccade` offset. Mirrors `LLEyeMotion::adjustEyeTarget`'s final composition.
fn eye_rotations(
    target: Quat,
    vergence: f32,
    aiming: bool,
    saccade: SaccadeOffset,
) -> (Quat, Quat) {
    // Jitter unless the eyes are strongly crossed on a very near target.
    let jitter = if vergence > VERGENCE_JITTER_THRESHOLD {
        Quat::from_euler(EulerRot::XYZ, 0.0, saccade.pitch, saccade.yaw)
    } else {
        Quat::IDENTITY
    };
    let vergence_quat = if aiming {
        Quat::from_axis_angle(Vec3::Z, vergence)
    } else {
        Quat::IDENTITY
    };
    let left = vergence_quat.mul_quat(jitter).mul_quat(target);
    // The right eye toes in the opposite way (the reference transposes/conjugates
    // the vergence quaternion).
    let right = vergence_quat.conjugate().mul_quat(jitter).mul_quat(target);
    (left, right)
}

/// Fold the head & eye look-at adjusters into `pose` in place: turn the neck and
/// head toward `look_dir` (an optional look-at direction in the avatar-local Second
/// Life frame) with the reference's lag and smoothing, then aim and jitter the
/// eyes. `neck_parent_world` is the neck parent joint's current **world** rotation
/// (avatar-local frame, with the animation + idle pose already folded in), so the
/// aim is computed against where the animated spine actually is rather than an
/// assumed rest pose. `interocular` is the eye separation (metres) for vergence;
/// `dt` is the frame time; `state` carries the per-avatar smoothing / saccade
/// accumulators. With no `look_dir` the neck / head relax back to their animation
/// pose and the eyes only jitter.
fn apply_to_pose(
    pose: &mut AnimationPose,
    look_dir: Option<Vec3>,
    interocular: f32,
    joints: LookAtJoints,
    neck_parent_world: Quat,
    dt: f32,
    state: &mut AgentLookAt,
) -> (f32, BlinkWeights) {
    // --- Head / neck (LLHeadRotMotion) ---
    // The desired head **world** rotation (avatar-local frame) to face the target,
    // then the rotation limit — applied **relative to the animated upper body**
    // (`neck_parent_world`), not the rest pose. The reference limits
    // `targetHeadRotWorld · ~currentRootRotWorld` for the same reason: an idle
    // animation that turns the body would otherwise make the symmetric rest-relative
    // limit look lopsided (the head turns far one way, stops short the other).
    let raw_target = look_dir.map_or(Quat::IDENTITY, head_target_rotation);
    let relative = neck_parent_world.inverse().mul_quat(raw_target);
    let head_target = neck_parent_world.mul_quat(constrain(relative, HEAD_ROTATION_CONSTRAINT));
    let head_slerp = smooth_interpolant(HEAD_LOOKAT_LAG_HALF_LIFE, dt);
    let aim = state.last_head_rot.lerp(head_target, head_slerp);
    state.last_head_rot = aim;

    // Ease an "aiming" weight in while there is a target and out when there is none,
    // so the neck / head blend between the animation keyframe (idle) and the
    // *absolute* look-at aim. Aiming **replaces** the keyframe for these joints (the
    // reference's head-track is a high-priority motion): folding a delta onto the
    // keyframe would let the aim drift with the animation's own head motion each
    // loop instead of staying on target.
    let target_weight = if look_dir.is_some() { 1.0 } else { 0.0 };
    let weight_slerp = smooth_interpolant(LOOK_AT_WEIGHT_HALF_LIFE, dt);
    state.weight += (target_weight - state.weight) * weight_slerp;

    // Distribute the world aim across the neck and head: the neck takes `NECK_LAG`
    // of it, the head completes it. Each joint's **local** rotation is derived from
    // its parent's actual world rotation, so the head lands on the aim regardless of
    // what the animation does to the spine between the root and the neck.
    let neck_world = Quat::IDENTITY.slerp(aim, NECK_LAG);
    let neck_local = neck_parent_world.inverse().mul_quat(neck_world);
    let head_local = neck_world.inverse().mul_quat(aim);
    set_blended(pose, joints.neck, neck_local, state.weight);
    set_blended(pose, joints.head, head_local, state.weight);

    // The head's world rotation (avatar-local frame) for converting the eye aim into
    // head-local space — the aim itself once engaged.
    let head_world = aim;

    // --- Eyes (LLEyeMotion): aim, jitter / look-away, and blink ---
    let eye = state.saccade.advance(dt, &mut state.rng);
    let (target, vergence, aiming) = match look_dir {
        Some(dir) => (
            eye_target_rotation(dir, head_world),
            eye_vergence(interocular, dir.length()),
            true,
        ),
        None => (Quat::IDENTITY, FOVEAL_OFFSET, false),
    };
    let (left, right) = eye_rotations(target, vergence, aiming, eye.offset);
    // The eyes rest at identity local rotation, so the aim replaces rather than
    // layers — but `compose` on an identity base is exactly that.
    compose(pose, joints.eye_left, left);
    compose(pose, joints.eye_right, right);
    compose(pose, joints.alt_eye_left, left);
    compose(pose, joints.alt_eye_right, right);

    // The head rotation angle (radians) effectively applied this frame — the aim
    // angle scaled by the eased weight — for diagnostics, plus the eyelid blink
    // morph weights for the per-frame runtime-morph pipeline (P31.12b).
    (Quat::IDENTITY.angle_between(aim) * state.weight, eye.blink)
}

/// Apply the head & eye look-at adjusters for one avatar, resolving its look-at
/// direction (Bevy target → avatar-local Second Life frame) from `targets`.
///
/// `head_pos` / `eye_positions` are the joint translations in the avatar-local
/// Second Life frame (from the deformed skeleton, before this fold); `root` is the
/// avatar-root global (its rotation maps avatar-local Second Life vectors into Bevy
/// world). Runs even without a target so the eyes keep their idle jitter and the
/// head smoothly returns to rest.
/// Debug switches read once per pose pass from the environment: force every
/// avatar's look-at to a fixed strong side/up direction (`SL_VIEWER_LOOK_AT_TEST`)
/// so the head-turn fold is unmistakable, and log each avatar's target / applied
/// head angle (`SL_VIEWER_LOG_LOOK_AT`).
#[derive(Clone, Copy, Default)]
pub(crate) struct LookAtDebug {
    /// A forced look-at direction (avatar-local Second Life frame) applied to every
    /// avatar, overriding its real target — for visibly confirming the fold works.
    force_dir: Option<Vec3>,
    /// Whether to log per-avatar look-at diagnostics.
    log: bool,
}

impl LookAtDebug {
    /// Read the debug switches from the environment.
    pub(crate) fn from_env() -> Self {
        let force_dir = if std::env::var("SL_VIEWER_LOOK_AT_TEST").as_deref() == Ok("1") {
            // Strongly to the avatar's left (Second Life +Y) and slightly up (+Z),
            // well past the head-rotation constraint so the crane is obvious.
            Some(Vec3::new(0.0, 5.0, 1.0))
        } else {
            None
        };
        Self {
            force_dir,
            log: std::env::var("SL_VIEWER_LOG_LOOK_AT").as_deref() == Ok("1"),
        }
    }
}

/// Apply the head & eye look-at adjusters for one avatar, resolving its look-at
/// direction (Bevy target → avatar-local Second Life frame) from `targets`.
#[expect(
    clippy::too_many_arguments,
    reason = "the look-at fold needs the avatar's target, root frame, joint set, \
              positions, per-avatar state and debug switches; grouping them into a \
              struct would only move the argument list"
)]
pub(crate) fn apply(
    pose: &mut AnimationPose,
    agent: AgentKey,
    targets: &LookAtTargets,
    motion: &mut LookAtMotion,
    root: &GlobalTransform,
    head_pos: Option<Vec3>,
    eye_positions: Option<(Vec3, Vec3)>,
    joints: LookAtJoints,
    neck_parent_world: Quat,
    dt: f32,
    debug: LookAtDebug,
) -> BlinkWeights {
    let real_dir = targets.point(agent).zip(head_pos).map(|(point, head)| {
        // Direction from the head to the target, in Bevy world space, rotated back
        // into the avatar-local Second Life frame the deformed skeleton uses.
        let head_bevy = root.transform_point(head);
        let dir_bevy = vsub(point, head_bevy);
        root.rotation().inverse().mul_vec3(dir_bevy)
    });
    let look_dir = debug.force_dir.or(real_dir);
    let interocular = eye_positions.map_or(0.0, |(left, right)| vsub(left, right).length());
    let (head_angle, blink) = apply_to_pose(
        pose,
        look_dir,
        interocular,
        joints,
        neck_parent_world,
        dt,
        motion.state_mut(agent),
    );
    if debug.log {
        let dir = look_dir.map(|d| (d.x, d.y, d.z));
        info!(
            "P31.12 look-at agent={agent} target={} dir_local={dir:?} head_angle={head_angle:.3}rad head_joint={:?} blink=({:.2},{:.2})",
            targets.point(agent).is_some(),
            joints.head,
            blink.left,
            blink.right,
        );
    }
    blink
}

/// Derive the own avatar's look-at target from the debug fly-camera: the camera's
/// own position, refreshed every frame, so the own avatar tracks (makes eye
/// contact with) the viewer's camera. The reference derives the own look-at from
/// the mouselook / cursor-focus point, which the free-fly debug camera has no
/// analog for; looking *at the camera* is the clearest, most recognisable stand-in
/// (as you orbit, the head and eyes follow you) and matches the "camera awareness"
/// glance idle viewers show. Does nothing until the agent id is known.
pub(crate) fn update_own_look_at_target(
    time: Res<Time>,
    identity: Res<SlIdentity>,
    camera: Query<&GlobalTransform, With<FlyCamera>>,
    mut targets: ResMut<LookAtTargets>,
) {
    let Some(own) = identity.agent_id else {
        return;
    };
    let Ok(transform) = camera.single() else {
        return;
    };
    targets.set(own, transform.translation(), time.elapsed_secs());
}

/// Ingest `ViewerEffect` look-at effects from nearby avatars into [`LookAtTargets`]
/// (P11-era gaze hints), and prune expired targets. The own avatar's gaze is driven
/// from the camera instead ([`update_own_look_at_target`]), so an echoed own-avatar
/// effect is ignored. A cleared / ended look-at drops the target.
pub(crate) fn receive_look_at_effects(
    time: Res<Time>,
    identity: Res<SlIdentity>,
    mut events: MessageReader<SlEvent>,
    mut targets: ResMut<LookAtTargets>,
) {
    let now = time.elapsed_secs();
    let own = identity.agent_id;
    let origin = identity.region_handle;
    for event in events.read() {
        let SlSessionEvent::ViewerEffect(effects) = &event.0 else {
            continue;
        };
        for effect in effects {
            let ViewerEffectData::LookAt {
                source,
                target_position,
                look_at_type,
                ..
            } = &effect.data
            else {
                continue;
            };
            let Some(source) = *source else {
                continue;
            };
            if own == Some(source) {
                continue;
            }
            // A cleared / ended gaze drops the avatar's target so its head returns
            // to rest.
            if matches!(look_at_type, LookAtType::None | LookAtType::Clear) {
                targets.clear(source);
                continue;
            }
            let Some(point) = global_to_bevy(*target_position, origin) else {
                continue;
            };
            targets.set(source, point, now);
        }
    }
    targets.prune(now);
}

/// Convert a global look-at target position into Bevy world space, using the
/// agent's current region south-west corner as the scene origin (matching the
/// terrain / coarse-avatar placement). Returns [`None`] until the region handle is
/// known.
fn global_to_bevy(
    target: sl_client_bevy::GlobalCoordinates,
    origin: Option<RegionHandle>,
) -> Option<Vec3> {
    let origin = origin?;
    let (corner_x, corner_y) = origin.global_coordinates();
    // Region-local Second Life metres = global − the origin region's SW corner.
    let local = sl_client_bevy::Vector {
        x: narrow(target.x()) - metres_to_f32(corner_x),
        y: narrow(target.y()) - metres_to_f32(corner_y),
        z: narrow(target.z()),
    };
    Some(sl_to_bevy_vec(&local))
}

/// Narrow a global-metre `f64` to the `f32` the scene works in. Global metres stay
/// well within `f32`'s exact-integer range once the region origin is subtracted, so
/// the visible precision loss is negligible; the truncation is unavoidable at the
/// `f64` → `f32` boundary.
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
        AgentLookAt, EYE_JITTER_MAX_PITCH, EYE_JITTER_MAX_YAW, EYE_LOOK_AWAY_MAX_PITCH,
        EYE_LOOK_AWAY_MAX_YAW, EYE_ROT_LIMIT_ANGLE, HEAD_ROTATION_CONSTRAINT,
        MIN_HEAD_LOOKAT_DISTANCE, Rng, Saccade, apply_to_pose, basis_rotation, constrain,
        eye_target_rotation, eye_vergence, head_target_rotation, smooth_interpolant,
    };
    use bevy::prelude::*;
    use sl_client_bevy::{AgentKey, AnimationPose, Uuid};

    /// Absolute-difference float check (the workspace forbids bare `==` on floats).
    fn near(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() <= eps, "{a} not within {eps} of {b}");
    }

    /// The rotation angle of a quaternion, radians in `[0, π]`.
    fn angle(q: Quat) -> f32 {
        Quat::IDENTITY.angle_between(q)
    }

    #[test]
    fn forward_look_at_is_identity_head() {
        // Looking straight ahead (Second Life +X forward) needs no head turn.
        let rot = head_target_rotation(Vec3::new(5.0, 0.0, 0.0));
        near(angle(rot), 0.0, 1e-5);
    }

    #[test]
    fn basis_forward_maps_x_to_forward() {
        // The basis rotation carries local +X onto the look direction.
        let dir = Vec3::new(0.0, 1.0, 0.0).normalize();
        let rot = basis_rotation(dir);
        let mapped = rot.mul_vec3(Vec3::X);
        assert!(
            mapped.abs_diff_eq(dir, 1e-5),
            "local +X mapped to {mapped:?}, expected {dir:?}"
        );
    }

    #[test]
    fn near_target_leaves_head_at_rest() {
        // Closer than the minimum look distance, the head stays at rest.
        let rot = head_target_rotation(Vec3::new(0.0, MIN_HEAD_LOOKAT_DISTANCE * 0.5, 0.0));
        near(angle(rot), 0.0, 1e-6);
    }

    #[test]
    fn head_aim_is_unconstrained_but_applied_aim_is_clamped() {
        // The raw head aim faces the target unclamped (a ~180° turn for a look
        // directly behind)…
        let raw = head_target_rotation(Vec3::new(-5.0, 0.0, 0.0));
        assert!(
            angle(raw) > HEAD_ROTATION_CONSTRAINT,
            "raw aim {} should be unconstrained",
            angle(raw)
        );
        // …and applying it (with an identity upper body) clamps the head+neck to the
        // limit — driving one frame to the target with a large slerp.
        let joints = super::LookAtJoints {
            neck: Some(1),
            head: Some(2),
            eye_left: Some(3),
            eye_right: Some(4),
            alt_eye_left: None,
            alt_eye_right: None,
        };
        let mut state = AgentLookAt::new(AgentKey::from(Uuid::from_u128(11)));
        let mut pose = AnimationPose::new();
        for _ in 0..600 {
            pose = AnimationPose::new();
            apply_to_pose(
                &mut pose,
                Some(Vec3::new(-5.0, 0.0, 0.0)),
                0.06,
                joints,
                Quat::IDENTITY,
                0.05,
                &mut state,
            );
        }
        let neck = pose.rotation(1).unwrap_or(Quat::IDENTITY);
        let head = pose.rotation(2).unwrap_or(Quat::IDENTITY);
        let combined = neck.mul_quat(head);
        assert!(
            angle(combined) <= HEAD_ROTATION_CONSTRAINT + 1e-2,
            "applied head+neck {} exceeds constraint {HEAD_ROTATION_CONSTRAINT}",
            angle(combined)
        );
    }

    #[test]
    fn limit_is_relative_to_the_animated_upper_body() {
        // The head limit is measured from the neck parent's (animated) rotation, not
        // the rest pose: a target dead ahead of a body turned 30° to the left leaves
        // the head near that body direction, not clamped as if it were 30° off rest.
        let body_turn = Quat::from_rotation_z(0.5);
        let joints = super::LookAtJoints {
            neck: Some(1),
            head: Some(2),
            eye_left: Some(3),
            eye_right: Some(4),
            alt_eye_left: None,
            alt_eye_right: None,
        };
        let mut state = AgentLookAt::new(AgentKey::from(Uuid::from_u128(12)));
        // Only the smoothed aim (`state.last_head_rot`) is asserted, so a single
        // scratch pose is reused across frames.
        let mut pose = AnimationPose::new();
        for _ in 0..600 {
            // Target straight ahead (Second Life +X) while the upper body faces +0.5 rad.
            apply_to_pose(
                &mut pose,
                Some(Vec3::new(5.0, 0.0, 0.0)),
                0.06,
                joints,
                body_turn,
                0.05,
                &mut state,
            );
        }
        // The head world aim relative to the body is small (facing forward within the
        // body's frame is well inside the limit, so no clamp distortion).
        let relative = body_turn.inverse().mul_quat(state.last_head_rot);
        assert!(
            angle(relative) <= HEAD_ROTATION_CONSTRAINT + 1e-3,
            "head relative to body {} should be within the limit",
            angle(relative)
        );
    }

    #[test]
    fn constrain_clamps_large_and_passes_small() {
        let big = Quat::from_rotation_z(2.0);
        let clamped = constrain(big, 1.0);
        near(angle(clamped), 1.0, 1e-5);
        let small = Quat::from_rotation_z(0.3);
        near(angle(constrain(small, 1.0)), 0.3, 1e-5);
    }

    #[test]
    fn smooth_interpolant_bounds() {
        // Zero frame time never moves; a long frame relative to the half-life moves
        // most of the way; the result always stays in [0, 1].
        near(smooth_interpolant(0.15, 0.0), 0.0, 1e-6);
        assert!(smooth_interpolant(0.15, 1.0) > 0.9);
        for dt in [0.0_f32, 0.001, 0.016, 0.1, 1.0, 10.0] {
            let f = smooth_interpolant(0.2, dt);
            assert!(
                (0.0..=1.0).contains(&f),
                "interpolant {f} out of range at dt={dt}"
            );
        }
    }

    #[test]
    fn one_half_life_moves_halfway() {
        // By definition of the half-life, one half-life of frame time moves half the
        // remaining distance.
        near(smooth_interpolant(0.2, 0.2), 0.5, 1e-6);
    }

    #[test]
    fn vergence_is_negative_and_bounded() {
        // A near target crosses the eyes inward (negative before the foveal offset),
        // a far one barely at all; always within a right angle.
        let near_v = eye_vergence(0.06, 0.5);
        let far_v = eye_vergence(0.06, 50.0);
        assert!(
            near_v < far_v,
            "near {near_v} should cross more than far {far_v}"
        );
        assert!(far_v.abs() < core::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn eye_target_is_constrained() {
        // Even an extreme look-at keeps the eye rotation within its limit.
        let rot = eye_target_rotation(Vec3::new(-1.0, 3.0, 0.0), Quat::IDENTITY);
        assert!(
            angle(rot) <= EYE_ROT_LIMIT_ANGLE + 1e-4,
            "eye angle {} exceeds limit {EYE_ROT_LIMIT_ANGLE}",
            angle(rot)
        );
    }

    #[test]
    fn saccade_stays_within_bounds() {
        // Over a long run the combined jitter + look-away never exceeds the summed
        // per-motion maxima.
        let mut saccade = Saccade::default();
        let mut rng = Rng::new(0x1234_5678);
        let yaw_bound = EYE_JITTER_MAX_YAW + EYE_LOOK_AWAY_MAX_YAW + 1e-6;
        let pitch_bound = EYE_JITTER_MAX_PITCH + EYE_LOOK_AWAY_MAX_PITCH + 1e-6;
        let mut moved = false;
        for _ in 0..5000 {
            let offset = saccade.advance(0.05, &mut rng).offset;
            assert!(
                offset.yaw.abs() <= yaw_bound,
                "yaw {} exceeds {yaw_bound}",
                offset.yaw
            );
            assert!(
                offset.pitch.abs() <= pitch_bound,
                "pitch {} exceeds {pitch_bound}",
                offset.pitch
            );
            if offset.yaw.abs() > 1e-6 {
                moved = true;
            }
        }
        assert!(moved, "saccade never produced any eye motion");
    }

    #[test]
    fn blink_weights_stay_in_range_and_the_eyes_actually_blink() {
        // Over a long run the eyelid morph weights never leave [0, 1], and the eyes
        // both fully close and fully reopen at least once.
        let mut saccade = Saccade::default();
        let mut rng = Rng::new(0x0BAD_F00D);
        let mut saw_fully_shut = false;
        let mut saw_fully_open_after_shut = false;
        for _ in 0..20_000 {
            let blink = saccade.advance(0.02, &mut rng).blink;
            for w in [blink.left, blink.right] {
                assert!(
                    (0.0..=1.0).contains(&w),
                    "blink weight {w} out of range [0, 1]"
                );
            }
            if blink.left >= 1.0 && blink.right >= 1.0 {
                saw_fully_shut = true;
            }
            if saw_fully_shut && blink.left <= 0.0 && blink.right <= 0.0 {
                saw_fully_open_after_shut = true;
            }
        }
        assert!(saw_fully_shut, "the eyes never fully closed");
        assert!(
            saw_fully_open_after_shut,
            "the eyes never reopened after a blink"
        );
    }

    #[test]
    fn a_blink_closes_the_right_eye_no_earlier_than_the_left() {
        // The right eyelid lags the left by EYE_BLINK_TIME_DELTA while closing, so it
        // is never further shut than the left during a blink's closing ramp.
        let mut saccade = Saccade::default();
        let mut rng = Rng::new(0x1357_9BDF);
        for _ in 0..2_000 {
            let blink = saccade.advance(0.005, &mut rng).blink;
            // While closing (not yet fully shut) the right must trail the left.
            if blink.left < 1.0 && !saccade.eyes_closed {
                assert!(
                    blink.right <= blink.left + 1e-6,
                    "right eye {} closed ahead of left {}",
                    blink.right,
                    blink.left
                );
            }
        }
    }

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            let x = a.unit();
            near(x, b.unit(), 0.0);
            assert!((0.0..1.0).contains(&x), "unit {x} out of range");
        }
    }

    #[test]
    fn apply_turns_head_toward_a_side_target() {
        // A look-at to the avatar's left (Second Life +Y) turns the head off rest,
        // and the smoothing converges over repeated frames.
        let joints = super::LookAtJoints {
            neck: Some(1),
            head: Some(2),
            eye_left: Some(3),
            eye_right: Some(4),
            alt_eye_left: None,
            alt_eye_right: None,
        };
        let mut state = AgentLookAt::new(AgentKey::from(Uuid::from_u128(7)));
        let mut pose = AnimationPose::new();
        for _ in 0..120 {
            pose = AnimationPose::new();
            apply_to_pose(
                &mut pose,
                Some(Vec3::new(0.0, 3.0, 0.0)),
                0.06,
                joints,
                Quat::IDENTITY,
                0.05,
                &mut state,
            );
        }
        // The head joint takes the remaining aim after the neck's share, so it turns
        // off rest.
        let head = pose.rotation(2).unwrap_or(Quat::IDENTITY);
        assert!(
            angle(head) > 0.05,
            "head did not turn toward the side target (angle {})",
            angle(head)
        );
        // With an identity neck parent, neck + head together reconstruct the full
        // constrained aim.
        let neck = pose.rotation(1).unwrap_or(Quat::IDENTITY);
        let combined = neck.mul_quat(head);
        assert!(
            angle(combined) > 0.5,
            "neck+head did not reach a large aim (angle {})",
            angle(combined)
        );
    }

    #[test]
    fn apply_without_target_relaxes_head_to_rest() {
        // With no look-at target the head settles back toward rest (identity),
        // leaving only the small eye jitter.
        let joints = super::LookAtJoints {
            neck: Some(1),
            head: Some(2),
            eye_left: Some(3),
            eye_right: Some(4),
            alt_eye_left: None,
            alt_eye_right: None,
        };
        let mut state = AgentLookAt::new(AgentKey::from(Uuid::from_u128(9)));
        // Seed a turned head, then run with no target.
        for _ in 0..30 {
            let mut pose = AnimationPose::new();
            apply_to_pose(
                &mut pose,
                Some(Vec3::new(0.0, 3.0, 0.0)),
                0.06,
                joints,
                Quat::IDENTITY,
                0.05,
                &mut state,
            );
        }
        let mut pose = AnimationPose::new();
        for _ in 0..300 {
            pose = AnimationPose::new();
            apply_to_pose(
                &mut pose,
                None,
                0.06,
                joints,
                Quat::IDENTITY,
                0.05,
                &mut state,
            );
        }
        let head = pose.rotation(2).unwrap_or(Quat::IDENTITY);
        near(angle(head), 0.0, 1e-2);
    }
}
