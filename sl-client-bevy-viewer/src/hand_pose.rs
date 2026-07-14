//! Hand-pose morph (P31.13): the always-on adjuster that shapes an avatar's hands
//! into the pose its playing animation asks for, cross-fading when that pose
//! changes — a port of the reference viewer's `LLHandMotion`.
//!
//! Second Life does not animate the finger joints from an animation's keyframe
//! tracks. Instead every `.anim` header carries a **hand pose** index (P18.1,
//! [`HandPose`]) naming one of fourteen fixed hand shapes, and the viewer expresses
//! that shape as an upper-body **morph**: one `Hands_*` visual param
//! ([`HAND_POSE_MORPH_PARAMS`]) at weight 1 while the others sit at 0. Changing pose
//! ramps the incoming morph up and the outgoing one down over
//! [`HAND_MORPH_BLEND_TIME`], so a fist opens into a point rather than snapping.
//!
//! Two pieces of machinery make this work here:
//!
//! - **Which pose is requested.** The reference has each playing keyframe motion
//!   publish its own hand pose (`applyKeyframes`) guarded by the motion's
//!   [`max_priority`](sl_anim::Motion::max_priority), so the highest-priority
//!   animation wins the hands;
//!   [`AnimationPlayback::requested_hand_pose`] resolves exactly that. With no
//!   decoded animation playing at all, the request is [`None`] and the hands fade
//!   to [`HandPose::RELAXED`] — the reference's resting default, not the base
//!   mesh's splayed `HAND_POSE_SPREAD`.
//!
//! - **How the morph is driven.** The hand morphs are per-frame
//!   [runtime morph params](sl_client_bevy::RUNTIME_MORPH_PARAMS) (P31.12a), so the
//!   cross-fade writes weights into the [`AvatarRuntimeMorphs`] resource and the
//!   GPU morph pipeline folds them onto the already-baked upper body — no re-bake
//!   per frame.
//!
//! The `HAND_POSE_SPREAD` pose (index 0) is special throughout, exactly as in the
//! reference: it is the base mesh's own hand shape and therefore has **no morph**,
//! so fading to or from it only moves the *other* pose's weight.
//!
//! Debugging:
//!
//! - `SL_VIEWER_HAND_POSE_TEST=<0..13>` forces every avatar's requested hand pose,
//!   overriding the playing animation (`3` = fist is the most unmistakable).
//! - `SL_VIEWER_LOG_HAND_POSE=1` logs each avatar's pose transitions and the
//!   morph weights the cross-fade drives.

use std::collections::HashMap;

use bevy::prelude::*;
use sl_anim::HandPose;
use sl_client_bevy::{AgentKey, HAND_POSE_MORPH_PARAMS, hand_pose_morph_param};

use crate::animations::{AnimationManager, AnimationPlayback};
use crate::avatars::{AvatarRuntimeMorphs, AvatarState};
use crate::reach::{EDITING_HAND_POSE, EDITING_HAND_POSE_PRIORITY, PointAtTargets};

/// Seconds one hand-pose morph takes to fully cross-fade into another (reference
/// `HAND_MORPH_BLEND_TIME`).
const HAND_MORPH_BLEND_TIME: f32 = 0.2;

/// The number of hand poses (reference `LLHandMotion::NUM_HAND_POSES`), i.e. the
/// length of the pose-indexed weight vector each avatar carries.
const NUM_HAND_POSES: usize = HAND_POSE_MORPH_PARAMS.len();

/// The hand pose an avatar rests in when no animation requests one (reference
/// `LLHandMotion`'s `mCurrentPose` / `mNewPose` initial value, and the pose it
/// falls back to whenever the `"Hand Pose"` animation data is absent).
const DEFAULT_HAND_POSE: HandPose = HandPose::RELAXED;

/// The spread hand pose (index 0) — the **base mesh's own** hand shape, which
/// therefore has no morph of its own. Blending to or from it only moves the other
/// pose's weight.
const SPREAD_HAND_POSE: HandPose = HandPose::SPREAD;

/// One avatar's hand-pose cross-fade state — the reference `LLHandMotion`'s
/// `mCurrentPose` / `mNewPose` plus the visual-param weights it drives (which the
/// reference keeps on the character rather than in the motion).
struct AgentHandPose {
    /// The pose the hands are currently *in* — the one whose morph is at (or is
    /// fading from) full weight. Equal to [`requested`](Self::requested) once a
    /// cross-fade completes.
    current: HandPose,
    /// The pose the hands are fading *towards* (reference `mNewPose`).
    requested: HandPose,
    /// Each pose's morph weight, indexed by pose. Index [`SPREAD_HAND_POSE`] is
    /// unused (that pose has no morph) and always stays 0.
    weights: [f32; NUM_HAND_POSES],
}

impl Default for AgentHandPose {
    /// The reference `LLHandMotion::onActivate`: every hand morph at 0 except the
    /// resting pose's, at full weight — so an avatar with no animation yet already
    /// has relaxed rather than splayed hands.
    fn default() -> Self {
        let mut state = Self {
            current: DEFAULT_HAND_POSE,
            requested: DEFAULT_HAND_POSE,
            weights: [0.0; NUM_HAND_POSES],
        };
        state.set_weight(DEFAULT_HAND_POSE, 1.0);
        state
    }
}

impl AgentHandPose {
    /// The morph weight of `pose` (0 for the morph-less spread pose).
    fn weight(&self, pose: HandPose) -> f32 {
        usize::try_from(pose.value())
            .ok()
            .and_then(|index| self.weights.get(index))
            .copied()
            .unwrap_or(0.0)
    }

    /// Set the morph weight of `pose`, ignoring the morph-less spread pose (whose
    /// slot must stay 0) and an out-of-range index.
    fn set_weight(&mut self, pose: HandPose, weight: f32) {
        if pose == SPREAD_HAND_POSE {
            return;
        }
        if let Ok(index) = usize::try_from(pose.value())
            && let Some(slot) = self.weights.get_mut(index)
        {
            *slot = weight;
        }
    }

    /// Advance the cross-fade by `dt` seconds towards the pose `requested` by the
    /// playing animation ([`None`] when no animation requests one), returning
    /// whether the pose the hands are in changed.
    ///
    /// A faithful port of `LLHandMotion::onUpdate`:
    ///
    /// - No request at all relaxes the hands ([`DEFAULT_HAND_POSE`]).
    /// - A request for a pose the reference does not define
    ///   ([`is_known`](HandPose::is_known) — an index a `.anim` header can legally
    ///   carry, since the decoder's bounds check is off by one, exactly as the
    ///   reference's is) is **ignored**: the hands keep heading wherever they were.
    /// - Re-requesting a pose the hands are *still fading away from* snaps both the
    ///   outgoing and the incoming morph back to their end weights before the new
    ///   fade starts, or a quick A → B → A would leave A's morph stranded half-way.
    fn update(&mut self, requested: Option<HandPose>, dt: f32) -> bool {
        match requested {
            Some(pose) if !pose.is_known() => {}
            _ => {
                let target = requested.unwrap_or(DEFAULT_HAND_POSE);
                if target != self.requested && self.requested != self.current {
                    self.set_weight(self.requested, 0.0);
                    self.set_weight(self.current, 1.0);
                }
                self.requested = target;
            }
        }
        if self.current == self.requested {
            return false;
        }
        // Ramp the incoming morph up and the outgoing one down. The spread pose has
        // no morph, so it counts as already fully faded on its side.
        let step = if HAND_MORPH_BLEND_TIME > 0.0 {
            dt / HAND_MORPH_BLEND_TIME
        } else {
            1.0
        };
        let incoming = if self.requested == SPREAD_HAND_POSE {
            1.0
        } else {
            let weight = (self.weight(self.requested) + step).clamp(0.0, 1.0);
            self.set_weight(self.requested, weight);
            weight
        };
        let outgoing = if self.current == SPREAD_HAND_POSE {
            0.0
        } else {
            let weight = (self.weight(self.current) - step).clamp(0.0, 1.0);
            self.set_weight(self.current, weight);
            weight
        };
        if incoming >= 1.0 && outgoing <= 0.0 {
            self.current = self.requested;
            return true;
        }
        false
    }
}

/// Per-avatar hand-pose cross-fade state, keyed by agent (P31.13). Retained across
/// frames so a pose change fades continuously; a rigged avatar with no entry yet
/// starts from [`AgentHandPose::default`] (relaxed hands).
#[derive(Resource, Default)]
pub(crate) struct HandPoseMotion {
    /// Each rigged avatar's hand-pose state.
    states: HashMap<AgentKey, AgentHandPose>,
}

/// Force every avatar's requested hand pose from `SL_VIEWER_HAND_POSE_TEST`
/// (a pose index, e.g. `3` for a fist), overriding the playing animation, so the
/// morph can be seen without hunting for content that requests an unusual pose.
fn forced_hand_pose() -> Option<HandPose> {
    let raw = std::env::var("SL_VIEWER_HAND_POSE_TEST").ok()?;
    HandPose::from_index(raw.trim().parse().ok()?)
}

/// Whether `SL_VIEWER_LOG_HAND_POSE` asks for the per-avatar hand-pose trace.
fn log_hand_pose() -> bool {
    std::env::var("SL_VIEWER_LOG_HAND_POSE").is_ok_and(|value| value != "0")
}

/// Drive every rigged avatar's hand-pose morphs (P31.13): resolve the pose its
/// highest-priority playing animation requests, advance the cross-fade towards it,
/// and publish the resulting morph weights to the per-frame runtime-morph pipeline
/// (P31.12a), which folds them onto the upper body's GPU morph targets.
///
/// Runs after [`drive_avatar_skeletons`](crate::animations::drive_avatar_skeletons)
/// (so this frame's playing set is already reconciled) and before
/// [`apply_avatar_runtime_morphs`](crate::avatars::apply_avatar_runtime_morphs) (so
/// the weights land in the same frame's `MeshMorphWeights`).
///
/// Every pose's weight is written each frame, not just the two the fade is moving,
/// so a pose the avatar left behind cannot linger at a stale weight.
pub(crate) fn drive_hand_poses(
    time: Res<Time>,
    state: Res<AvatarState>,
    playback: Res<AnimationPlayback>,
    manager: Res<AnimationManager>,
    point_at: Res<PointAtTargets>,
    mut motion: ResMut<HandPoseMotion>,
    mut runtime_morphs: ResMut<AvatarRuntimeMorphs>,
) {
    let dt = time.delta_secs();
    let forced = forced_hand_pose();
    let log = log_hand_pose();
    let agents = state.rigged_agents();
    for &agent in &agents {
        // The editing reach (P31.15) asks for its own hand shape while it reaches, at its
        // own priority — the reference's `LLEditingMotion::sHandPose`.
        let editing = point_at
            .is_editing(agent)
            .then_some((EDITING_HAND_POSE_PRIORITY, EDITING_HAND_POSE));
        let requested = forced.or_else(|| playback.requested_hand_pose(agent, &manager, editing));
        let hands = motion.states.entry(agent).or_default();
        let changed = hands.update(requested, dt);
        for (index, param) in HAND_POSE_MORPH_PARAMS.into_iter().enumerate() {
            let Some(param) = param else {
                continue;
            };
            let weight = hands.weights.get(index).copied().unwrap_or(0.0);
            runtime_morphs.set(agent, param, weight);
        }
        if log && changed {
            debug!(
                "hand pose: avatar {agent} settled into pose {} ({})",
                hands.current.value(),
                hand_pose_morph_param(
                    usize::try_from(hands.current.value()).unwrap_or(NUM_HAND_POSES)
                )
                .unwrap_or("<spread: base mesh>")
            );
        }
        if log {
            let driven: Vec<String> = HAND_POSE_MORPH_PARAMS
                .into_iter()
                .enumerate()
                .filter_map(|(index, param)| {
                    let weight = hands.weights.get(index).copied().unwrap_or(0.0);
                    (weight > 0.0).then(|| format!("{}={weight:.2}", param.unwrap_or("spread")))
                })
                .collect();
            trace!(
                "hand pose: avatar {agent} requested={:?} current={} [{}]",
                requested.map(HandPose::value),
                hands.current.value(),
                driven.join(" ")
            );
        }
    }
    // Forget avatars that are no longer rigged (despawned / left the region), so a
    // returning one starts from relaxed hands rather than a stale mid-fade.
    motion.states.retain(|agent, _hands| agents.contains(agent));
}

#[cfg(test)]
mod tests {
    use super::{AgentHandPose, HAND_MORPH_BLEND_TIME, NUM_HAND_POSES, SPREAD_HAND_POSE};
    use pretty_assertions::assert_eq;
    use sl_anim::HandPose;

    /// Run the cross-fade for `seconds` at a fixed 60 Hz step with a constant
    /// request, returning the number of steps taken.
    fn run(hands: &mut AgentHandPose, requested: Option<HandPose>, seconds: f32) {
        let dt = 1.0 / 60.0;
        let mut elapsed = 0.0;
        while elapsed < seconds {
            let _changed = hands.update(requested, dt);
            elapsed += dt;
        }
    }

    #[test]
    fn rests_in_the_relaxed_pose() {
        let hands = AgentHandPose::default();
        assert_eq!(hands.current, HandPose::RELAXED);
        assert!(hands.weight(HandPose::RELAXED) > 0.99);
        assert!(hands.weight(HandPose::FIST).abs() < f32::EPSILON);
        // The spread pose has no morph, so its slot is never driven.
        assert!(hands.weight(SPREAD_HAND_POSE).abs() < f32::EPSILON);
    }

    #[test]
    fn cross_fades_between_two_poses() {
        let mut hands = AgentHandPose::default();
        // Half a blend in: the fist is fading in, the relaxed pose out, and neither
        // has arrived — the hands are genuinely between the two.
        run(
            &mut hands,
            Some(HandPose::FIST),
            HAND_MORPH_BLEND_TIME / 2.0,
        );
        let fist = hands.weight(HandPose::FIST);
        let relaxed = hands.weight(HandPose::RELAXED);
        assert!(fist > 0.2 && fist < 0.8, "mid-fade fist weight {fist}");
        assert!(
            relaxed > 0.2 && relaxed < 0.8,
            "mid-fade relaxed weight {relaxed}"
        );
        assert_eq!(hands.current, HandPose::RELAXED);
        // Past the full blend time the fist has arrived and the relaxed morph is gone.
        run(&mut hands, Some(HandPose::FIST), HAND_MORPH_BLEND_TIME);
        assert_eq!(hands.current, HandPose::FIST);
        assert!(hands.weight(HandPose::FIST) > 0.99);
        assert!(hands.weight(HandPose::RELAXED).abs() < f32::EPSILON);
    }

    #[test]
    fn no_request_falls_back_to_the_relaxed_pose() {
        let mut hands = AgentHandPose::default();
        run(
            &mut hands,
            Some(HandPose::POINT),
            HAND_MORPH_BLEND_TIME * 2.0,
        );
        assert_eq!(hands.current, HandPose::POINT);
        // The animation stops: with nothing requesting a pose the hands relax again.
        run(&mut hands, None, HAND_MORPH_BLEND_TIME * 2.0);
        assert_eq!(hands.current, HandPose::RELAXED);
        assert!(hands.weight(HandPose::RELAXED) > 0.99);
        assert!(hands.weight(HandPose::POINT).abs() < f32::EPSILON);
    }

    #[test]
    fn spread_pose_only_fades_the_other_morph() {
        let mut hands = AgentHandPose::default();
        // Fading to the spread pose just fades the relaxed morph out — spread *is*
        // the base mesh, so no morph of its own ever rises.
        run(
            &mut hands,
            Some(SPREAD_HAND_POSE),
            HAND_MORPH_BLEND_TIME * 2.0,
        );
        assert_eq!(hands.current, SPREAD_HAND_POSE);
        assert!(
            hands
                .weights
                .iter()
                .all(|weight| weight.abs() < f32::EPSILON)
        );
        // And back out of it: only the incoming morph rises.
        run(
            &mut hands,
            Some(HandPose::FIST),
            HAND_MORPH_BLEND_TIME * 2.0,
        );
        assert_eq!(hands.current, HandPose::FIST);
        assert!(hands.weight(HandPose::FIST) > 0.99);
    }

    #[test]
    fn re_requesting_the_fading_pose_resets_both_morphs() {
        let mut hands = AgentHandPose::default();
        // Interrupt a relaxed → fist fade half-way by asking for relaxed again: the
        // reference snaps both morphs back to their end weights so the fist does not
        // stay stranded mid-blend.
        run(
            &mut hands,
            Some(HandPose::FIST),
            HAND_MORPH_BLEND_TIME / 2.0,
        );
        assert!(hands.weight(HandPose::FIST) > 0.2);
        let _changed = hands.update(Some(HandPose::RELAXED), 1.0 / 60.0);
        assert_eq!(hands.current, HandPose::RELAXED);
        assert_eq!(hands.requested, HandPose::RELAXED);
        assert!(hands.weight(HandPose::FIST).abs() < f32::EPSILON);
        assert!(hands.weight(HandPose::RELAXED) > 0.99);
    }

    #[test]
    fn weights_stay_normalised_through_a_long_pose_sequence() {
        let mut hands = AgentHandPose::default();
        for pose in [
            HandPose::FIST,
            HandPose::POINT,
            HandPose::TYPING,
            HandPose::SALUTE_R,
            HandPose::RELAXED,
        ] {
            run(&mut hands, Some(pose), HAND_MORPH_BLEND_TIME * 2.0);
            assert_eq!(hands.current, pose);
            // Exactly one morph is driven at a time once a fade settles, and every
            // weight stays a valid morph weight throughout.
            let driven = hands.weights.iter().filter(|w| **w > 0.0).count();
            assert_eq!(driven, 1, "settled on pose {}", pose.value());
            assert!(
                hands
                    .weights
                    .iter()
                    .all(|weight| (0.0..=1.0).contains(weight))
            );
        }
        assert_eq!(hands.weights.len(), NUM_HAND_POSES);
    }
}
