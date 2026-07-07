//! Sampling a decoded keyframe [`Motion`] into per-joint Bevy poses — the P18.3
//! skeleton-driver seam, the animation counterpart of `to_bevy_base_mesh` /
//! `to_bevy_mesh`.
//!
//! `sl-anim` decodes an `.anim` into per-joint keyframe tracks and interpolates
//! them (Bevy-free, in Second Life Z-up joint-local space); this thin adapter
//! samples every animated joint of a motion at a playback time and hands back the
//! values as Bevy [`Quat`] / [`Vec3`], leaving the ECS work (advancing playback
//! time, resolving joint names to skeleton-instance entities, resolving which
//! motion wins a joint) to the viewer.
//!
//! The values stay in SL joint-local space: the single SL → Bevy axis change is
//! carried at the avatar root (as for the base-body geometry), so a driver writes
//! [`rotation`](SampledJoint::rotation) / [`position`](SampledJoint::position)
//! directly onto the joint's local [`Transform`](bevy::transform::components::Transform).
//! A keyframe rotation is the joint's absolute local rotation (the animatable
//! `m*` joints rest at identity), so it *replaces* the joint's rest rotation
//! rather than composing with it.

use bevy::math::{Quat, Vec3};
use sl_anim::Motion;

/// One joint's sampled local pose from a keyframe motion at a playback time.
///
/// A joint may be animated in only one channel; the unanimated channel is `None`
/// so the driver leaves that part of the joint's rest transform untouched.
#[derive(Debug, Clone, Copy)]
pub struct SampledJoint<'a> {
    /// The animated joint's name (`mPelvis`, `mChest`, …), which the caller
    /// resolves against the target skeleton to find the joint entity to pose.
    pub name: &'a str,
    /// The joint's effective priority in this motion (its own, or the motion's
    /// base priority when the joint defers) — used to resolve which motion wins
    /// the joint when several play at once.
    pub priority: i32,
    /// The sampled local rotation (SL Z-up), when the joint has a rotation track.
    pub rotation: Option<Quat>,
    /// The sampled local position (SL Z-up, metres), when the joint has a
    /// position track.
    pub position: Option<Vec3>,
}

/// Sample every animated joint of `motion` at `elapsed` seconds since the motion
/// started, converting the Z-up keyframe values to Bevy [`Quat`] / [`Vec3`] (still
/// in SL joint-local space).
///
/// The time *within* the motion is derived from `elapsed` via
/// [`Motion::playback_time`], so the loop points are honoured. A joint animated in
/// neither channel is omitted. The `.anim` stores each rotation as `[x, y, z, w]`,
/// matching [`Quat::from_xyzw`].
#[must_use]
pub fn sample_motion(motion: &Motion, elapsed: f32) -> Vec<SampledJoint<'_>> {
    let time = motion.playback_time(elapsed);
    motion
        .joints
        .iter()
        .filter_map(|joint| {
            let rotation = joint
                .sample_rotation(time)
                .map(|[x, y, z, w]| Quat::from_xyzw(x, y, z, w));
            let position = joint
                .sample_position(time)
                .map(|[x, y, z]| Vec3::new(x, y, z));
            if rotation.is_none() && position.is_none() {
                return None;
            }
            Some(SampledJoint {
                name: &joint.name,
                priority: joint.effective_priority(motion.base_priority),
                rotation,
                position,
            })
        })
        .collect()
}
