//! Blending several motions' contributions to one joint by priority (P18.4) —
//! the pure counterpart of the reference viewer's `LLJointStateBlender`.
//!
//! When an avatar plays more than one animation at once (an animation-overrider
//! stand under a gesture, say), each joint may be driven by several motions. The
//! reference viewer resolves that per joint: it keeps the top few contributions
//! ordered by priority (recency breaking ties), then folds them highest-priority
//! first, letting each fill up the remaining weight budget until the joint's
//! weight reaches one. A higher-priority motion therefore dominates a joint while
//! a lower-priority one only shows through the weight the winner leaves unfilled
//! (and while a motion eases in / out, its partial [`weight`](JointContribution::weight)
//! lets the next one bleed through).
//!
//! This module is the Bevy-free maths only: the caller (the `sl-client-bevy`
//! driver) samples each motion, resolves joint names to skeleton indices, and
//! computes each motion's ease-in/out [`pose_weight`](crate::Motion::pose_weight),
//! then hands the per-joint contributions here in Second Life Z-up joint-local
//! space (`[f32; 4]` / `[f32; 3]`), exactly as [`sample`](crate::Motion) keeps the
//! single-motion path renderer-agnostic.
//!
//! Faithful reproduction notes (mirrors `LLJointStateBlender::addJointState` /
//! `blendJointStates`):
//!
//! - Only the top [`MAX_JOINT_CONTRIBUTIONS`] contributions (by priority, then
//!   recency) blend; the rest are dropped, as the reference keeps four slots.
//! - Ties in priority are broken by **recency**: the most-recently-activated
//!   motion wins, because the reference pushes each newly-started motion to the
//!   front of its active list and inserts equal priorities without displacing the
//!   ones already there. The caller supplies that recency as
//!   [`order`](JointContribution::order) (higher = more recent). This reproduces
//!   Second Life's quirk that, among equal-priority animations, the last one
//!   *started* wins for an observer present when it started, whereas an observer
//!   arriving later — who starts them all at once in the simulator's set order —
//!   gets the set-order winner instead.
//! - A zero-weight contribution is skipped (an animation fully eased out but not
//!   yet dropped), never overwriting a lower-priority one.
//! - The rotation blend uses the same `nlerp` (short-arc) the single-motion
//!   sampler does; the position blend uses a plain component lerp. Scale is not
//!   animated by a `.anim` keyframe track, so only rotation and position blend.

use crate::sample::{lerp_vector3, nlerp_quaternions};

/// The number of per-joint contributions the reference viewer blends
/// (`JSB_NUM_JOINT_STATES`); further, lower-priority contributions are dropped.
pub const MAX_JOINT_CONTRIBUTIONS: usize = 4;

/// One motion's contribution to a single joint this frame: its effective
/// priority, its activation recency, its ease-in/out weight, and the sampled
/// local pose (either channel absent when the motion does not animate it).
///
/// Values are in Second Life Z-up joint-local space, matching the single-motion
/// [`sample`](crate::Motion) path.
#[derive(Clone, Copy, Debug)]
pub struct JointContribution {
    /// The contributing motion's effective priority for this joint (its own joint
    /// priority, or the motion's base priority when the joint defers).
    pub priority: i32,
    /// The contributing motion's activation recency: higher means more recently
    /// started, breaking ties in `priority`. See the module note on how this
    /// reproduces Second Life's equal-priority ordering.
    pub order: u64,
    /// The motion's ease-in/out pose weight in `0..=1` this frame.
    pub weight: f32,
    /// The sampled local rotation `[x, y, z, w]` (SL Z-up), when the motion
    /// animates this joint's rotation.
    pub rotation: Option<[f32; 4]>,
    /// The sampled local position `[x, y, z]` (SL Z-up, metres), when the motion
    /// animates this joint's position.
    pub position: Option<[f32; 3]>,
}

/// The blended local pose for one joint: the rotation / position resolved from
/// every contribution, each channel absent when no contribution animates it (so
/// the driver leaves that part of the joint's rest transform untouched).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlendedJoint {
    /// The blended local rotation `[x, y, z, w]` (SL Z-up), if any contribution
    /// animated rotation.
    pub rotation: Option<[f32; 4]>,
    /// The blended local position `[x, y, z]` (SL Z-up, metres), if any
    /// contribution animated position.
    pub position: Option<[f32; 3]>,
}

/// Blend one joint's `contributions` into a single local pose, mirroring
/// `LLJointStateBlender::blendJointStates`.
///
/// The slice is first ordered by priority (recency breaking ties) and capped to
/// [`MAX_JOINT_CONTRIBUTIONS`]; then each channel is folded highest-priority
/// first. The first contributor to a channel is copied outright and the running
/// weight-sum set to its weight; each later one fills the remaining budget —
/// `new_sum = min(1, weight + sum)`, blending the accumulated (higher-priority)
/// value toward the new (lower-priority) one by `sum / new_sum` so the winner keeps
/// the larger share. A zero-weight contribution is skipped.
///
/// The `contributions` slice is reordered in place (the caller owns a scratch
/// `Vec` it can reuse); the return value is the blended pose.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "`blend_joint` names the pose-blend operation clearly; the `blend` module groups the P18.4 blender"
)]
pub fn blend_joint(contributions: &mut [JointContribution]) -> BlendedJoint {
    // Priority descending, recency (order) descending for ties — the reference's
    // slot order, where a newly-started motion sits ahead of equal-priority ones
    // already present.
    contributions.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| b.order.cmp(&a.order))
    });
    let capped = contributions
        .get(..MAX_JOINT_CONTRIBUTIONS)
        .unwrap_or(contributions);

    let mut rotation: Option<[f32; 4]> = None;
    let mut sum_rotation = 0.0_f32;
    let mut position: Option<[f32; 3]> = None;
    let mut sum_position = 0.0_f32;

    for contribution in capped {
        if contribution.weight <= 0.0 {
            continue;
        }
        if let Some(rot) = contribution.rotation {
            match rotation {
                None => {
                    rotation = Some(rot);
                    sum_rotation = contribution.weight;
                }
                Some(accumulated) => {
                    let new_sum = (contribution.weight + sum_rotation).min(1.0);
                    // `nlerp(sum / new_sum, incoming, accumulated)`: at the limit
                    // the higher-priority `accumulated` dominates.
                    let fraction = sum_rotation / new_sum;
                    rotation = Some(nlerp_quaternions(fraction, rot, accumulated));
                    sum_rotation = new_sum;
                }
            }
        }
        if let Some(pos) = contribution.position {
            match position {
                None => {
                    position = Some(pos);
                    sum_position = contribution.weight;
                }
                Some(accumulated) => {
                    let new_sum = (contribution.weight + sum_position).min(1.0);
                    let fraction = sum_position / new_sum;
                    position = Some(lerp_vector3(fraction, pos, accumulated));
                    sum_position = new_sum;
                }
            }
        }
    }

    BlendedJoint { rotation, position }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A contribution with only a rotation track.
    fn rot(priority: i32, order: u64, weight: f32, rotation: [f32; 4]) -> JointContribution {
        JointContribution {
            priority,
            order,
            weight,
            rotation: Some(rotation),
            position: None,
        }
    }

    #[track_caller]
    fn approx4(actual: [f32; 4], expected: [f32; 4]) {
        for (a, e) in actual.iter().zip(expected.iter()) {
            assert!(
                (a - e).abs() < 1e-6,
                "expected ~{expected:?}, got {actual:?}"
            );
        }
    }

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap`/`expect` (the crate convention, mirroring `sl-anim`'s decode
    /// tests).
    type TestResult = Result<(), Box<dyn core::error::Error>>;

    #[test]
    fn empty_contributions_blend_to_nothing() {
        assert_eq!(blend_joint(&mut []), BlendedJoint::default());
    }

    #[test]
    fn single_full_weight_contribution_is_copied() -> TestResult {
        let mut contributions = [rot(0, 0, 1.0, [0.0, 0.0, 1.0, 0.0])];
        let blended = blend_joint(&mut contributions);
        approx4(blended.rotation.ok_or("rotation")?, [0.0, 0.0, 1.0, 0.0]);
        Ok(())
    }

    #[test]
    fn a_lone_easing_in_contribution_still_applies_fully() -> TestResult {
        // The first contributor to a channel is copied outright regardless of its
        // weight (the reference never blends a lone motion toward the rest pose).
        let mut contributions = [rot(0, 0, 0.25, [0.0, 0.0, 1.0, 0.0])];
        let blended = blend_joint(&mut contributions);
        approx4(blended.rotation.ok_or("rotation")?, [0.0, 0.0, 1.0, 0.0]);
        Ok(())
    }

    #[test]
    fn higher_priority_wins_the_joint_outright_at_full_weight() -> TestResult {
        // A full-weight high-priority motion leaves no budget, so the low-priority
        // one does not show through.
        let identity = [0.0, 0.0, 0.0, 1.0];
        let z_ninety = [0.0, 0.0, 0.707_106_77, 0.707_106_77];
        let mut contributions = [
            rot(0, 0, 1.0, identity), // low priority
            rot(2, 1, 1.0, z_ninety), // high priority
        ];
        let blended = blend_joint(&mut contributions);
        approx4(blended.rotation.ok_or("rotation")?, z_ninety);
        Ok(())
    }

    #[test]
    fn equal_priority_breaks_ties_by_recency() -> TestResult {
        // Two equal-priority, full-weight motions: the more recently activated
        // (higher `order`) wins outright, since it sorts first and fills the whole
        // weight budget.
        let older = [0.0, 0.0, 0.0, 1.0];
        let newer = [0.0, 0.0, 1.0, 0.0];
        let mut contributions = [rot(1, 5, 1.0, older), rot(1, 9, 1.0, newer)];
        let blended = blend_joint(&mut contributions);
        approx4(blended.rotation.ok_or("rotation")?, newer);
        Ok(())
    }

    #[test]
    fn only_the_top_contributions_blend() -> TestResult {
        // A fifth, distinct contribution beyond MAX_JOINT_CONTRIBUTIONS is dropped:
        // four identical full-weight identities plus one outlier still resolves to
        // identity because the outlier is the lowest priority and falls off.
        let identity = [0.0, 0.0, 0.0, 1.0];
        let outlier = [1.0, 0.0, 0.0, 0.0];
        let mut contributions = [
            rot(4, 4, 1.0, identity),
            rot(3, 3, 1.0, identity),
            rot(2, 2, 1.0, identity),
            rot(1, 1, 1.0, identity),
            rot(0, 0, 1.0, outlier),
        ];
        assert!(contributions.len() > MAX_JOINT_CONTRIBUTIONS);
        let blended = blend_joint(&mut contributions);
        approx4(blended.rotation.ok_or("rotation")?, identity);
        Ok(())
    }

    #[test]
    fn a_zero_weight_contribution_is_skipped() -> TestResult {
        // A fully-eased-out (weight 0) high-priority motion must not overwrite the
        // live lower-priority one.
        let live = [0.0, 0.0, 1.0, 0.0];
        let faded = [1.0, 0.0, 0.0, 0.0];
        let mut contributions = [rot(2, 9, 0.0, faded), rot(0, 1, 1.0, live)];
        let blended = blend_joint(&mut contributions);
        approx4(blended.rotation.ok_or("rotation")?, live);
        Ok(())
    }

    #[test]
    fn position_blends_by_weight_budget() -> TestResult {
        // Two equal-priority position contributions, the high-recency one at half
        // weight: it copies first (sum 0.5), then the other fills the rest, so the
        // result is the weighted mix.
        let mut contributions = [
            JointContribution {
                priority: 0,
                order: 9,
                weight: 0.5,
                rotation: None,
                position: Some([2.0, 0.0, 0.0]),
            },
            JointContribution {
                priority: 0,
                order: 1,
                weight: 1.0,
                rotation: None,
                position: Some([0.0, 0.0, 0.0]),
            },
        ];
        let blended = blend_joint(&mut contributions);
        let position = blended.position.ok_or("position")?;
        // new_sum = min(1, 1.0 + 0.5) = 1.0; fraction = 0.5 / 1.0 = 0.5:
        // lerp(0.5, [0,0,0], [2,0,0]) = [1,0,0].
        assert!((position[0] - 1.0).abs() < 1e-6, "got {position:?}");
        Ok(())
    }
}
