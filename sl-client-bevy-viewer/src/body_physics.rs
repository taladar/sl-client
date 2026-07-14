//! Body physics (P34.2): the per-frame half of the avatar cloth & body physics —
//! the reference viewer's `LLPhysicsMotionController`, one spring-damper per
//! breast / belly / butt axis, driven by the acceleration of the joint the body
//! part hangs off (`mChest` / `mPelvis`).
//!
//! [[viewer-p34-1]] ingested the `WT_PHYSICS` wearable into a [`BodyPhysics`] per
//! avatar; the simulation itself lives in `sl-avatar`
//! ([`BodyPhysicsState`], pure and frame-agnostic). This module is the seam: it
//! samples where each motion's joint currently is, steps the springs, and folds the
//! result back into the avatar the two ways the reference does:
//!
//! - **The system body** bounces through the `*_Driven` **morph params**, written
//!   into the per-frame runtime-morph pipeline (P31.12a) — the morph targets
//!   themselves are the ones [[viewer-p34-1]] synthesizes at `.llm` load.
//! - **A worn rigged-mesh body** bounces through the driven params' **volume
//!   morphs**, which displace the `LEFT_PEC` / `RIGHT_PEC` / `BELLY` / `BUTT`
//!   **collision volumes**. Since P17.2 those are bindable joints, so a mesh body
//!   rigged to them follows — and a morph target on the system body could never
//!   reach it. They are folded in as [`AnimationPose`] position deltas on the
//!   volume joints, at the same seam the look-at / locomotion / reach adjusters use.
//!
//! The motion is sampled in **Bevy world space** — the joint's world position
//! composed through the avatar-root global, so the avatar's own travel through the
//! region is part of the forcing term exactly as the reference's
//! `LLJoint::getWorldPosition` makes it. The Second Life → Bevy axis change is a
//! proper rotation, so a dot product along the motion's axis is the same number in
//! either frame; only the world-up vector has to be named per frame, and here that
//! is `+Y`.
//!
//! # Debugging
//!
//! - `SL_VIEWER_PHYSICS_TEST=1` forces every motion's `Max_Effect` to `1`, the
//!   reference viewer's own `physics_test` switch. `Max_Effect` **defaults to zero
//!   on every axis**, so an avatar that does not wear a tuned physics wearable — the
//!   OpenSim test avatar included — never bounces at all, and this is what makes the
//!   simulation visible without one.
//! - `SL_VIEWER_LOG_BODY_PHYSICS=1` logs each avatar's per-motion simulated position
//!   and the weight it is driving its morphs to.

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, AnimationPose, BodyPhysics, BodyPhysicsState, JointSample, PhysicsMotion,
};

use crate::avatars::AvatarRuntimeMorphs;

/// Per-avatar body-physics simulation state, keyed by agent (P34.2). Retained across
/// frames — the springs are stateful — and dropped when an avatar despawns.
#[derive(Resource, Default, Debug)]
pub(crate) struct BodyPhysicsMotion {
    /// The running simulation of each rigged avatar seen so far.
    states: HashMap<AgentKey, BodyPhysicsState>,
}

impl BodyPhysicsMotion {
    /// Forget every avatar not in `live` (they have despawned).
    pub(crate) fn retain(&mut self, live: &impl Fn(AgentKey) -> bool) {
        self.states.retain(|&agent, _state| live(agent));
    }
}

/// What one avatar's body-physics step reads: its ingested wearable, the pose its
/// joints currently stand in, and where that pose sits in the world.
pub(crate) struct BodyPhysicsInput<'a> {
    /// The avatar being simulated.
    pub(crate) agent: AgentKey,
    /// The avatar's ingested physics wearable (P34.1).
    pub(crate) physics: &'a BodyPhysics,
    /// Its joint **world** matrices in the avatar's own Second Life frame, for the
    /// pose as it stands after the keyframe, idle and look-at folds — the same
    /// `world0` the locomotion and reach adjusters read.
    pub(crate) world: &'a [Mat4],
    /// The avatar-root global: the Second Life → Bevy axis change plus the avatar's
    /// placement in the world, which is what makes the joint sample include the
    /// avatar's own travel.
    pub(crate) root: &'a GlobalTransform,
    /// This frame's duration, in seconds.
    pub(crate) dt: f32,
}

/// What one avatar's body-physics step did, for the live diagnostic.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BodyPhysicsReport {
    /// How many motions are switched on (a zero `Max_Effect` is off).
    pub(crate) active: usize,
    /// The breast up-down motion's simulated position, in the normalized `0.0..=1.0`
    /// param space (`0.5` is the avatar's own shape) — the one number that says
    /// whether the springs are actually moving.
    pub(crate) breast_up_down: f32,
    /// The belly up-down motion's simulated position, likewise.
    pub(crate) belly_up_down: f32,
    /// The butt up-down motion's simulated position, likewise.
    pub(crate) butt_up_down: f32,
}

/// Step one avatar's body physics and fold the result into its `pose` (the collision
/// volumes a rigged mesh body rides) and its runtime `morphs` (the system body's
/// morph targets).
///
/// A motion that is switched off has its driven params released back to the avatar's
/// own shape rather than left where the last bounce put them, so taking a physics
/// wearable off settles the body instead of freezing it mid-bounce.
pub(crate) fn apply(
    pose: &mut AnimationPose,
    motion: &mut BodyPhysicsMotion,
    morphs: &mut AvatarRuntimeMorphs,
    input: &BodyPhysicsInput,
    joint_index: impl Fn(&str) -> Option<usize>,
) -> BodyPhysicsReport {
    let physics = input.physics;
    if !physics.is_active() {
        // Nothing to simulate: drop the springs and release every driven param back
        // to its appearance-resolved rest weight (a no-override param falls back to
        // exactly that), so a wearable that is taken off or tuned to zero settles.
        if motion.states.remove(&input.agent).is_some() {
            for config in physics.motions() {
                for driven in &config.driven {
                    morphs.clear(input.agent, &driven.name);
                }
            }
        }
        return BodyPhysicsReport::default();
    }
    let state = motion.states.entry(input.agent).or_default();
    // The avatar-root global's rotation carries a direction in the avatar's own
    // Second Life frame into Bevy world space.
    let root_rotation = input.root.rotation();
    state.step(physics, input.dt, Vec3::Y.to_array(), |motion| {
        let index = joint_index(motion.joint())?;
        let matrix = input.world.get(index)?;
        let (_scale, rotation, translation) = matrix.to_scale_rotation_translation();
        let direction = root_rotation
            .mul_quat(rotation)
            .mul_vec3(Vec3::from(motion.direction()));
        Some(JointSample {
            position: input.root.transform_point(translation).to_array(),
            direction: direction.to_array(),
        })
    });
    // The system body's morph targets (P31.12a runtime morphs).
    for (param, weight) in state.driven_weights(physics) {
        morphs.set(input.agent, param, weight);
    }
    // A rigged mesh body's collision volumes, as pose position deltas on the volume
    // joints (the pose position track is itself an offset from the joint's rest, which
    // is exactly what a volume morph is).
    for (volume, offset) in state.volume_offsets(physics) {
        if let Some(index) = joint_index(volume) {
            pose.set_position(index, Vec3::from(offset));
        }
    }
    BodyPhysicsReport {
        active: physics
            .motions()
            .iter()
            .filter(|config| config.settings.is_active())
            .count(),
        breast_up_down: state.position(PhysicsMotion::BreastUpDown),
        belly_up_down: state.position(PhysicsMotion::BellyUpDown),
        butt_up_down: state.position(PhysicsMotion::ButtUpDown),
    }
}

/// The live diagnostic (env `SL_VIEWER_LOG_BODY_PHYSICS=1`): whether to log each
/// avatar's simulated motion positions.
#[must_use]
pub(crate) fn log_enabled() -> bool {
    std::env::var("SL_VIEWER_LOG_BODY_PHYSICS").as_deref() == Ok("1")
}

/// The reference viewer's `physics_test` switch (env `SL_VIEWER_PHYSICS_TEST=1`):
/// whether to force every motion's `Max_Effect` on, so an avatar wearing no tuned
/// physics wearable — where every `Max_Effect` is zero and nothing would move —
/// still bounces.
#[must_use]
pub(crate) fn force_enabled() -> bool {
    std::env::var("SL_VIEWER_PHYSICS_TEST").as_deref() == Ok("1")
}

/// The `Max_Effect` [`force_enabled`] forces every motion to (the reference's
/// `behavior_maxeffect = 1.0f`): the driven params then sweep their whole range.
pub(crate) const FORCED_MAX_EFFECT: f32 = 1.0;
