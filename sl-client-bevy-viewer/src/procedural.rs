//! Always-on procedural idle pose adjusters (P31.8).
//!
//! The reference viewer stacks a set of *procedural* motions on top of every
//! avatar's sampled keyframe pose — motions that are never signalled over
//! `AvatarAnimation` and carry no downloadable asset, but run continuously on
//! every avatar to keep it from standing perfectly frozen. This module ports the
//! two that need **no external input** — no look-at target, no inverse
//! kinematics, no morph visual-params — so they apply faithfully to *every*
//! rendered avatar from time alone:
//!
//! - **Breathe** (`LLBreatheMotionRot`, `llvoavatar.cpp`): a slow sine pitch of
//!   the `mChest` joint about its local Y axis.
//! - **Body noise** (`LLBodyNoiseMotion`, `llvoavatar.cpp`): a subtle
//!   low-frequency sway of the `mTorso` joint about its local X / Y axes.
//!
//! Both are composed as a **small delta on top of** whatever the resolved
//! keyframe pose already animates for that joint (P18.3 [`AnimationPose`]), so a
//! playing animation still dominates the joint and the idle motion only shows
//! through where the joint is otherwise at rest — the same effect the reference
//! achieves with the additive / low-priority blend of these motions.
//!
//! **Not ported here** (deferred within P31.8): the adjusters that need state
//! this module has no access to — `LLHeadRotMotion` / `LLEyeMotion` (a world
//! look-at target), `LLHandMotion` (hand-pose morph animations),
//! `LLKeyframeStandMotion`'s lower-body twist to the look direction and the
//! `LLKeyframeWalkMotion` / `LLWalkAdjustMotion` foot-plant IK (inverse
//! kinematics), `LLFlyAdjustMotion` / `LLKeyframeFallMotion`, and the
//! activity-driven `LLEditingMotion` / `LLTargetingMotion` reach/aim. The
//! reference also gates each by avatar pixel area; that culling is not modelled
//! (the pose pass already runs only for rigged avatars).

use bevy::prelude::*;
use sl_client_bevy::AnimationPose;

/// Peak chest pitch of the breathing motion, radians. Ported verbatim from the
/// reference viewer's `LLBreatheMotionRot` (`BREATHE_ROT_MOTION_STRENGTH`); the
/// breathe rate is 1, so the chest pitches by `sin(time) * this`.
const BREATHE_ROT_MOTION_STRENGTH: f32 = 0.05;

/// Torso idle-sway amplitude, degrees, from `LLBodyNoiseMotion`
/// (`TORSO_NOISE_AMOUNT` — "amount of deviation from up-axis, in degrees").
const TORSO_NOISE_AMOUNT_DEG: f32 = 1.0;

/// Time-scale of the torso idle sway, from `LLBodyNoiseMotion`
/// (`TORSO_NOISE_SPEED`).
const TORSO_NOISE_SPEED: f32 = 0.2;

/// The breathing chest rotation at motion time `time` (seconds): a slow sine
/// pitch about the chest joint's local Y axis, exactly the reference
/// `LLBreatheMotionRot::onUpdate` (`sinf(rate * time) * strength`, rate = 1).
#[must_use]
pub(crate) fn breathe_rotation(time: f32) -> Quat {
    let breathe_amt = time.sin() * BREATHE_ROT_MOTION_STRENGTH;
    Quat::from_axis_angle(Vec3::Y, breathe_amt)
}

/// One band-limited noise stream in roughly `[-1, 1]`, cast-free: a weighted sum
/// of three incommensurate sines whose absolute weights total one. Two decorrelated
/// streams are drawn by `phase`.
///
/// The reference `LLBodyNoiseMotion` samples 2D Perlin `noise2`, but its
/// permutation / gradient tables are filled by `init()` with `llrand` and
/// **re-seeded every viewer startup**, so there is no canonical body-noise
/// waveform to reproduce bit-for-bit — only the *character* of the motion (a
/// smooth, slow, low-frequency wander). Given that, a full permutation-table port
/// buys nothing over this trig sum, which reproduces the character
/// deterministically for far less code. (The port is also not quite cast-free:
/// most of `noise2` is int↔int lookups masked to `u8` plus float residuals, but
/// turning the coordinate into its integer lattice-cell index is a float→int
/// truncation, and Rust std has no `From`/`TryFrom` for that — only `as`
/// (saturating) or `unsafe to_int_unchecked` — so it would need a line-scoped
/// `#[expect(clippy::as_conversions, …)]`.) The fundamental period (~5 s at
/// `TORSO_NOISE_SPEED`) matches the reference's one-lattice-cell timescale.
fn body_noise_component(t: f32, phase: f32) -> f32 {
    // Fundamental at one lattice cell (~5 s at `TORSO_NOISE_SPEED`), plus two
    // incommensurate harmonics for a non-repeating wander.
    let a = (t * core::f32::consts::TAU + phase).sin();
    let b = (t * 13.7 + phase * 1.7).sin();
    let c = (t * 3.1 + phase * 0.5).sin();
    a * 0.6 + b * 0.25 + c * 0.15
}

/// The torso idle-sway rotation at motion time `time` (seconds): small pitches
/// about the torso joint's local X and Y axes driven by two decorrelated
/// low-frequency noise streams, mirroring `LLBodyNoiseMotion::onUpdate`
/// (`tQn.setQuat(rx, ry, 0)` with `rx`/`ry` from `noise2` along each axis).
#[must_use]
pub(crate) fn body_noise_rotation(time: f32) -> Quat {
    let amount = TORSO_NOISE_AMOUNT_DEG.to_radians();
    let t = time * TORSO_NOISE_SPEED;
    let rx = amount * body_noise_component(t, 0.0);
    let ry = amount * body_noise_component(t, 2.0);
    Quat::from_rotation_x(rx).mul_quat(Quat::from_rotation_y(ry))
}

/// Fold the always-on idle adjusters into a resolved pose in place: a slow
/// breathing pitch on `mChest` and a subtle low-frequency sway on `mTorso`, each
/// composed as a small delta on top of whatever the keyframe pose already
/// animates for that joint (so a playing animation still dominates and the idle
/// motion only shows where the joint is otherwise at rest). `joint_index` maps a
/// joint name to the skeleton index the pose is keyed by; a joint the skeleton
/// lacks is skipped.
pub(crate) fn apply_idle_adjustments(
    pose: &mut AnimationPose,
    time: f32,
    joint_index: impl Fn(&str) -> Option<usize>,
) {
    if let Some(chest) = joint_index("mChest") {
        let base = pose.rotation(chest).unwrap_or(Quat::IDENTITY);
        pose.set_rotation(chest, base.mul_quat(breathe_rotation(time)));
    }
    if let Some(torso) = joint_index("mTorso") {
        let base = pose.rotation(torso).unwrap_or(Quat::IDENTITY);
        pose.set_rotation(torso, base.mul_quat(body_noise_rotation(time)));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BREATHE_ROT_MOTION_STRENGTH, TORSO_NOISE_AMOUNT_DEG, apply_idle_adjustments,
        body_noise_rotation, breathe_rotation,
    };
    use bevy::prelude::*;
    use sl_client_bevy::AnimationPose;

    /// Absolute-difference float check (the workspace forbids bare `==` on floats).
    fn near(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() <= eps, "{a} not within {eps} of {b}");
    }

    /// The rotation angle of a quaternion, radians in `[0, π]`.
    fn angle(q: Quat) -> f32 {
        Quat::IDENTITY.angle_between(q)
    }

    /// Assert two rotations are the same orientation. Compares by the dot product
    /// (`|a·b| = 1` for the same rotation, sign-agnostic since `q` and `-q` are the
    /// same rotation) rather than [`Quat::angle_between`], whose `acos` amplifies a
    /// one-ULP float difference near identical inputs into a ~1e-3 rad phantom angle.
    fn same_rotation(a: Quat, b: Quat) {
        near(a.dot(b).abs(), 1.0, 1e-6);
    }

    #[test]
    fn breathe_rest_is_identity_and_peaks_at_strength() {
        // sin(0) = 0 → no chest pitch at t = 0.
        near(angle(breathe_rotation(0.0)), 0.0, 1e-6);
        // sin(π/2) = 1 → the chest pitches by the full strength.
        near(
            angle(breathe_rotation(core::f32::consts::FRAC_PI_2)),
            BREATHE_ROT_MOTION_STRENGTH,
            1e-5,
        );
        // The breathe is a pure pitch about the local Y axis.
        let q = breathe_rotation(core::f32::consts::FRAC_PI_2);
        let axis_angle = q.to_axis_angle();
        near(axis_angle.0.dot(Vec3::Y).abs(), 1.0, 1e-4);
    }

    #[test]
    fn body_noise_stays_within_amplitude() {
        // Each component is bounded by the 1° amplitude, so the combined X/Y
        // rotation never exceeds ~√2° — a subtle sway, never a lurch. Sample a
        // long span so every phase of the slow noise is covered.
        let amount = TORSO_NOISE_AMOUNT_DEG.to_radians();
        let bound = amount * core::f32::consts::SQRT_2 + 1e-3;
        let mut time = 0.0_f32;
        while time < 120.0 {
            assert!(
                angle(body_noise_rotation(time)) <= bound,
                "sway {} exceeds bound {bound} at t={time}",
                angle(body_noise_rotation(time)),
            );
            time += 0.1;
        }
    }

    #[test]
    fn body_noise_moves_over_time() {
        // The sway is not frozen: it visibly differs a few seconds apart.
        let early = body_noise_rotation(1.0);
        let later = body_noise_rotation(4.0);
        assert!(
            early.angle_between(later) > 1e-3,
            "body noise did not move between samples",
        );
    }

    #[test]
    fn adjustments_compose_on_top_of_the_keyframe_pose() {
        // A joint the keyframe pose already animates keeps that rotation as the
        // base, with the idle delta layered on top (base · delta).
        let chest_index = 0;
        let torso_index = 1;
        let index = |name: &str| match name {
            "mChest" => Some(chest_index),
            "mTorso" => Some(torso_index),
            _ => None,
        };
        let base_chest = Quat::from_rotation_x(0.3);
        let mut pose = AnimationPose::new();
        pose.set_rotation(chest_index, base_chest);

        let time = core::f32::consts::FRAC_PI_2;
        apply_idle_adjustments(&mut pose, time, index);

        let expected_chest = base_chest.mul_quat(breathe_rotation(time));
        let got_chest = pose.rotation(chest_index).unwrap_or(Quat::IDENTITY);
        same_rotation(got_chest, expected_chest);
        // The idle delta actually moved the chest off its keyframe base.
        assert!(base_chest.dot(got_chest).abs() < 1.0 - 1e-6);

        // The torso had no keyframe rotation, so it starts from identity.
        let expected_torso = body_noise_rotation(time);
        let got_torso = pose.rotation(torso_index).unwrap_or(Quat::IDENTITY);
        same_rotation(got_torso, expected_torso);
    }

    #[test]
    fn missing_joints_are_skipped() {
        // A skeleton without the adjuster joints leaves the pose untouched.
        let mut pose = AnimationPose::new();
        apply_idle_adjustments(&mut pose, 1.0, |_name| None);
        assert!(
            pose.is_empty(),
            "pose should be untouched when joints are absent"
        );
    }
}
