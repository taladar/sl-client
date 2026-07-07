//! Sampling a decoded [`Motion`] at a playback time — the pure keyframe
//! interpolation a skeleton driver runs each frame (P18.3).
//!
//! [`Motion::from_bytes`](crate::Motion::from_bytes) decodes the `.anim` file
//! into per-joint keyframe tracks; this module turns those tracks back into a
//! posed skeleton. Given the seconds elapsed since a motion started, it maps that
//! to the time *within* the motion (honouring the loop points, exactly as the
//! reference viewer's `LLKeyframeMotion::onUpdate` does), then interpolates each
//! joint's rotation / position curve at that time
//! (`RotationCurve::getValue` / `PositionCurve::getValue`).
//!
//! The results stay in Second Life's right-handed **Z-up** joint-local space, as
//! plain `[f32; 4]` / `[f32; 3]` arrays — the crate is Bevy-free, so the
//! conversion to a renderer's quaternion / vector type (and the axis change at
//! the avatar root) lives in the `sl-client-bevy` driver, mirroring how
//! `sl-mesh` / `sl-prim` geometry is converted there rather than here.
//!
//! A keyframe rotation is the joint's *absolute* local rotation, not a delta: the
//! standard skeleton's animatable `m*` joints all rest at identity rotation, and
//! the reference pose blender (`LLJointStateBlender::blendJointStates`) copies the
//! keyframe value straight into the joint's local rotation, so a driver applies a
//! sampled value by replacing the joint's rest rotation with it (and, for the few
//! joints with a position track — chiefly `mPelvis` — its rest translation).

use crate::decode::{JointMotion, JointPriority, Motion};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the playback-time methods live in their own `sample` module, apart from the `decode` module's constructor impl"
)]
impl Motion {
    /// Map `elapsed` seconds since the motion started to the time *within* the
    /// motion to sample, honouring the loop points.
    ///
    /// Mirrors the playing (non-stopped) branch of `LLKeyframeMotion::onUpdate`:
    /// a non-looping motion samples straight at `elapsed`; a looping motion, once
    /// past its [`loop_out_point`](Self::loop_out_point), wraps back into the
    /// `[loop_in_point, loop_out_point]` window. A zero-duration looping motion
    /// samples at time `0`.
    #[must_use]
    pub fn playback_time(&self, elapsed: f32) -> f32 {
        let time = elapsed.max(0.0);
        if !self.loops {
            return time;
        }
        if self.duration == 0.0 {
            return 0.0;
        }
        if time <= self.loop_out_point {
            return time;
        }
        let span = self.loop_out_point - self.loop_in_point;
        if span == 0.0 {
            return self.loop_out_point;
        }
        // `time - loop_out_point` is non-negative here and `span` positive, so the
        // float remainder matches the reference `fmod`.
        self.loop_in_point + (time - self.loop_out_point) % span
    }

    /// Whether a non-looping motion has fully played out at `elapsed` seconds —
    /// past its duration and its ease-out tail — so a driver can stop sampling a
    /// finished one-shot (a wave, a bow) and let the joint fall back to its rest
    /// pose even before the simulator drops it from the avatar's animation list.
    /// A looping motion is never finished.
    #[must_use]
    pub fn is_expired(&self, elapsed: f32) -> bool {
        !self.loops && elapsed > self.duration + self.ease_out_duration
    }
}

impl JointMotion {
    /// The joint's effective animation priority given its motion's `base`
    /// priority: a joint tagged [`USE_MOTION`](JointPriority::USE_MOTION) inherits
    /// the motion's base priority, otherwise its own priority stands. Used to
    /// resolve which motion wins a joint when several play at once.
    #[must_use]
    pub const fn effective_priority(&self, base: JointPriority) -> i32 {
        if self.priority.value() == JointPriority::USE_MOTION.value() {
            base.value()
        } else {
            self.priority.value()
        }
    }

    /// The interpolated local rotation `[x, y, z, w]` (SL Z-up) at `time` seconds
    /// within the motion, or `None` when the joint has no rotation track.
    #[must_use]
    pub fn sample_rotation(&self, time: f32) -> Option<[f32; 4]> {
        sample_curve(
            &self.rotation_keys,
            time,
            |key| key.time,
            |key| key.rotation,
            nlerp_quaternions,
        )
    }

    /// The interpolated local position `[x, y, z]` (SL Z-up, metres) at `time`
    /// seconds within the motion, or `None` when the joint has no position track.
    #[must_use]
    pub fn sample_position(&self, time: f32) -> Option<[f32; 3]> {
        sample_curve(
            &self.position_keys,
            time,
            |key| key.time,
            |key| key.position,
            lerp_vector3,
        )
    }
}

/// Interpolate a keyframe curve at `time`, mirroring the reference viewer's
/// `RotationCurve::getValue` / `PositionCurve::getValue`: clamp to the first / last
/// key outside the track's range, return an exact key on a hit, and otherwise
/// `interp` between the two bracketing keys by the normalized in-between fraction.
/// The keys are assumed ascending in time (the file format stores them so, as does
/// the reference viewer's time-keyed map). Returns `None` for an empty track.
fn sample_curve<K: Copy, V>(
    keys: &[K],
    time: f32,
    key_time: impl Fn(K) -> f32,
    key_value: impl Fn(K) -> V,
    interp: impl Fn(f32, V, V) -> V,
) -> Option<V> {
    // The first key at or after `time` — the reference viewer's `lower_bound`.
    let right_index = keys.iter().position(|key| key_time(*key) >= time);
    let Some(right_index) = right_index else {
        // Past the last key: hold the final value.
        return keys.last().copied().map(key_value);
    };
    let right = keys.get(right_index).copied()?;
    // Before the first key: clamp to it. (An exact key hit needs no special case:
    // it falls out of the interpolation below at `fraction == 1.0`, which returns
    // the right key's value — avoiding a strict float-equality test.)
    let Some(left_index) = right_index.checked_sub(1) else {
        return Some(key_value(right));
    };
    let left = keys.get(left_index).copied()?;
    let before = key_time(left);
    let after = key_time(right);
    let span = after - before;
    if span == 0.0 {
        return Some(key_value(right));
    }
    let fraction = (time - before) / span;
    Some(interp(fraction, key_value(left), key_value(right)))
}

/// Normalized quaternion interpolation `[x, y, z, w]`, matching the reference
/// viewer's `nlerp(u, a, b)`: a plain component lerp when the quaternions face the
/// same hemisphere, else a true spherical interpolation that takes the short arc.
fn nlerp_quaternions(fraction: f32, a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    let dot = ax * bx + ay * by + az * bz + aw * bw;
    if dot < 0.0 {
        slerp_quaternions(fraction, a, b)
    } else {
        lerp_quaternions(fraction, a, b)
    }
}

/// Component-wise quaternion lerp then renormalize, matching the reference
/// viewer's `lerp(t, p, q)` (`inv_t * p + t * q`, normalized).
fn lerp_quaternions(fraction: f32, a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    let inv = 1.0 - fraction;
    normalize_quaternion([
        inv * ax + fraction * bx,
        inv * ay + fraction * by,
        inv * az + fraction * bz,
        inv * aw + fraction * bw,
    ])
}

/// Spherical quaternion interpolation, matching the reference viewer's
/// `slerp(u, a, b)` — including its opposite-hemisphere flip so the short arc is
/// taken, and its lerp fallback when the two orientations are nearly identical.
fn slerp_quaternions(fraction: f32, a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    let raw_cos = ax * bx + ay * by + az * bz + aw * bw;
    let flip = raw_cos < 0.0;
    let cos_theta = if flip { -raw_cos } else { raw_cos };
    let (mut beta, alpha) = if 1.0 - cos_theta < 0.00001 {
        // Nearly identical: fall back to a straight lerp.
        (1.0 - fraction, fraction)
    } else {
        let theta = cos_theta.acos();
        let sin_theta = theta.sin();
        (
            (theta - fraction * theta).sin() / sin_theta,
            (fraction * theta).sin() / sin_theta,
        )
    };
    if flip {
        beta = -beta;
    }
    [
        beta * ax + alpha * bx,
        beta * ay + alpha * by,
        beta * az + alpha * bz,
        beta * aw + alpha * bw,
    ]
}

/// Normalize a quaternion `[x, y, z, w]`, returning the identity rotation when its
/// length is degenerate (zero / non-finite) rather than producing `NaN` values.
fn normalize_quaternion(q: [f32; 4]) -> [f32; 4] {
    let [x, y, z, w] = q;
    let length_sq = x * x + y * y + z * z + w * w;
    if length_sq > 0.0 {
        let inv = 1.0 / length_sq.sqrt();
        [x * inv, y * inv, z * inv, w * inv]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// Component-wise vector lerp `a + fraction * (b - a)`, matching the reference
/// viewer's `lerp(a, b, u)` for a position track.
fn lerp_vector3(fraction: f32, a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [
        ax + fraction * (bx - ax),
        ay + fraction * (by - ay),
        az + fraction * (bz - az),
    ]
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::decode::{JointMotion, JointPriority, Motion, PositionKey, RotationKey};

    /// Assert two `f32`s agree within a tight tolerance — the crate convention for
    /// float comparisons (avoids the `float_cmp` lint the exact `assert_eq` trips).
    #[track_caller]
    fn approx(actual: f32, expected: f32) {
        let diff = (actual - expected).abs();
        assert!(
            diff < 1e-6,
            "expected ~{expected}, got {actual} (|diff| = {diff})"
        );
    }

    /// Assert two `[f32; N]` arrays agree component-wise within tolerance.
    #[track_caller]
    fn approx_slice(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len(), "differing lengths");
        for (a, e) in actual.iter().zip(expected.iter()) {
            approx(*a, *e);
        }
    }

    /// Build a bare looping-or-not [`Motion`] with the given loop window for the
    /// [`Motion::playback_time`] tests; the joint tracks are irrelevant here.
    fn timing_motion(loops: bool, duration: f32, loop_in: f32, loop_out: f32) -> Motion {
        Motion {
            base_priority: JointPriority::LOW,
            duration,
            emote_name: String::new(),
            loop_in_point: loop_in,
            loop_out_point: loop_out,
            loops,
            ease_in_duration: 0.0,
            ease_out_duration: 0.0,
            hand_pose: crate::decode::HandPose::RELAXED,
            joints: Vec::new(),
            constraints: Vec::new(),
        }
    }

    #[test]
    fn playback_time_non_looping_is_elapsed() {
        let motion = timing_motion(false, 4.0, 0.0, 4.0);
        approx(motion.playback_time(2.5), 2.5);
        // Past the end a non-looping motion keeps advancing (the caller / expiry
        // decides when to stop); the curve sampler holds the last key.
        approx(motion.playback_time(10.0), 10.0);
        // A negative elapsed clamps to zero.
        approx(motion.playback_time(-1.0), 0.0);
    }

    #[test]
    fn playback_time_loops_within_window() {
        let motion = timing_motion(true, 4.0, 1.0, 3.0);
        // Before the loop-out point: straight through.
        approx(motion.playback_time(0.5), 0.5);
        approx(motion.playback_time(3.0), 3.0);
        // Past loop-out: wrap into [loop_in, loop_out] (span 2.0).
        approx(motion.playback_time(3.5), 1.5);
        approx(motion.playback_time(5.0), 1.0);
        approx(motion.playback_time(6.5), 2.5);
    }

    #[test]
    fn playback_time_zero_duration_loop() {
        let motion = timing_motion(true, 0.0, 0.0, 0.0);
        approx(motion.playback_time(3.0), 0.0);
    }

    #[test]
    fn is_expired_only_for_finished_one_shot() {
        let one_shot = timing_motion(false, 2.0, 0.0, 2.0);
        assert!(!one_shot.is_expired(2.0));
        assert!(one_shot.is_expired(2.5));
        let looping = timing_motion(true, 2.0, 0.0, 2.0);
        assert!(!looping.is_expired(100.0));
    }

    /// A single-track joint used by the sampling tests.
    fn joint_with_rotations(keys: Vec<RotationKey>) -> JointMotion {
        JointMotion {
            name: "mPelvis".to_owned(),
            priority: JointPriority::USE_MOTION,
            rotation_keys: keys,
            position_keys: Vec::new(),
        }
    }

    #[test]
    fn sample_rotation_empty_track_is_none() {
        let joint = joint_with_rotations(Vec::new());
        assert!(joint.sample_rotation(1.0).is_none());
    }

    #[test]
    fn sample_rotation_exact_and_clamped_keys() -> Result<(), Box<dyn core::error::Error>> {
        let joint = joint_with_rotations(vec![
            RotationKey {
                time: 0.0,
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            RotationKey {
                time: 2.0,
                rotation: [0.0, 0.0, 1.0, 0.0],
            },
        ]);
        // Before the first key clamps to it.
        approx_slice(
            &joint.sample_rotation(-1.0).ok_or("before-first key")?,
            &[0.0, 0.0, 0.0, 1.0],
        );
        // Exactly on a key returns it.
        approx_slice(
            &joint.sample_rotation(2.0).ok_or("on-key")?,
            &[0.0, 0.0, 1.0, 0.0],
        );
        // Past the last key holds it.
        approx_slice(
            &joint.sample_rotation(9.0).ok_or("past-last key")?,
            &[0.0, 0.0, 1.0, 0.0],
        );
        Ok(())
    }

    #[test]
    fn sample_rotation_midpoint_is_unit_and_between() -> Result<(), Box<dyn core::error::Error>> {
        let joint = joint_with_rotations(vec![
            RotationKey {
                time: 0.0,
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            RotationKey {
                time: 2.0,
                rotation: [0.0, 0.0, 0.707_106_77, 0.707_106_77],
            },
        ]);
        let sampled = joint
            .sample_rotation(1.0)
            .ok_or("the joint has a rotation track")?;
        let [x, y, z, w] = sampled;
        // Still a unit quaternion.
        let length = (x * x + y * y + z * z + w * w).sqrt();
        assert!((length - 1.0).abs() < 1e-5, "not unit: {length}");
        // Halfway between identity and a +Z quarter turn stays in the +Z / +W
        // quadrant, strictly between the endpoints.
        assert!(z > 0.0 && z < 0.707_106_77);
        assert!(w > 0.707_106_77 && w < 1.0);
        approx(x, 0.0);
        approx(y, 0.0);
        Ok(())
    }

    #[test]
    fn sample_position_lerps_linearly() -> Result<(), Box<dyn core::error::Error>> {
        let joint = JointMotion {
            name: "mPelvis".to_owned(),
            priority: JointPriority::LOW,
            rotation_keys: Vec::new(),
            position_keys: vec![
                PositionKey {
                    time: 0.0,
                    position: [0.0, 0.0, 0.0],
                },
                PositionKey {
                    time: 4.0,
                    position: [4.0, -2.0, 8.0],
                },
            ],
        };
        approx_slice(
            &joint.sample_position(1.0).ok_or("has position track")?,
            &[1.0, -0.5, 2.0],
        );
        assert!(joint.sample_rotation(1.0).is_none());
        Ok(())
    }

    #[test]
    fn effective_priority_inherits_motion_base() {
        let use_motion = joint_with_rotations(Vec::new());
        assert_eq!(
            use_motion.effective_priority(JointPriority::HIGH),
            JointPriority::HIGH.value()
        );
        let own = JointMotion {
            name: "mChest".to_owned(),
            priority: JointPriority::HIGHEST,
            rotation_keys: Vec::new(),
            position_keys: Vec::new(),
        };
        assert_eq!(
            own.effective_priority(JointPriority::LOW),
            JointPriority::HIGHEST.value()
        );
    }
}
