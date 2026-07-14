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
    ///
    /// This is the priority-blind, stop-blind check; the blending driver (P18.4)
    /// uses the stop-aware [`is_finished`](Self::is_finished) instead, so it can
    /// hold a motion through its ease-out tail after the simulator drops it.
    #[must_use]
    pub fn is_expired(&self, elapsed: f32) -> bool {
        !self.loops && elapsed > self.duration + self.ease_out_duration
    }

    /// The motion's **pose weight** in `0..=1` at `elapsed` seconds since it
    /// started (P18.4), given `stopped_at` — the elapsed time at which the
    /// simulator dropped it from the avatar's animation set, or `None` while it
    /// is still signalled.
    ///
    /// Mirrors the per-frame `posep->setWeight(...)` of
    /// `LLMotionController::updateMotionsByType`: a cubic ease-in from activation
    /// over [`ease_in_duration`](Motion::ease_in_duration), full weight while
    /// active, and a cubic ease-out around the stop over
    /// [`ease_out_duration`](Motion::ease_out_duration). A non-looping motion
    /// auto-eases-out so it finishes at its [`duration`](Motion::duration) (the
    /// reference viewer's `mSendStopTimestamp` sits an ease-out before the end),
    /// and an explicit `stopped_at` brings that forward when it is earlier. The
    /// residual weight the ease-out scales is the ease-in weight reached at the
    /// stop, so stopping mid-ease-in fades from the partial weight rather than
    /// popping to full.
    #[must_use]
    pub fn pose_weight(&self, elapsed: f32, stopped_at: Option<f32>) -> f32 {
        let ease_in = self.ease_in_weight(elapsed);
        let Some(start) = self.ease_out_start(stopped_at) else {
            return ease_in;
        };
        if elapsed < start {
            return ease_in;
        }
        if self.ease_out_duration <= 0.0 {
            return 0.0;
        }
        // `cubic_step` clamps its argument, so a past-the-tail time yields 0.
        let residual = self.ease_in_weight(start);
        let fraction = (elapsed - start) / self.ease_out_duration;
        residual * cubic_step(1.0 - fraction)
    }

    /// The motion's **hand-pose arbitration priority**
    /// (`LLJointMotionList::mMaxPriority`): the highest *explicit* joint priority
    /// the file carries, i.e. ignoring joints that defer to the motion's base
    /// priority with [`USE_MOTION`](JointPriority::USE_MOTION).
    ///
    /// The reference viewer uses this — and only this — to decide whose
    /// [`hand_pose`](Motion::hand_pose) wins when several motions play at once
    /// (`LLKeyframeMotion::applyKeyframes` publishes its hand pose only when its
    /// `mMaxPriority` is at least the pose priority already published this frame),
    /// which is why it is a motion-wide scalar rather than the per-joint
    /// [`effective_priority`](JointMotion::effective_priority) the pose blend uses.
    ///
    /// Faithful to the reference's derivation: it starts at
    /// [`LOW`](JointPriority::LOW) — the motion's base priority does **not** raise
    /// it — and each joint with an explicit priority above it lifts it. The one
    /// exception is a file whose raw base priority reached
    /// [`ADDITIVE`](JointPriority::ADDITIVE), which the reference (and
    /// [`Motion::from_bytes`]) clamps to
    /// [`ADDITIVE_CLAMPED`](JointPriority::ADDITIVE_CLAMPED) and *does* seed
    /// `mMaxPriority` from; that clamped value is above every named joint priority,
    /// so it is recovered here by recognising it.
    #[must_use]
    pub fn max_priority(&self) -> JointPriority {
        let mut max = if self.base_priority == JointPriority::ADDITIVE_CLAMPED {
            self.base_priority
        } else {
            JointPriority::LOW
        };
        for joint in &self.joints {
            if joint.priority != JointPriority::USE_MOTION && joint.priority > max {
                max = joint.priority;
            }
        }
        max
    }

    /// Whether the motion has fully eased out at `elapsed` (its pose weight has
    /// reached 0 and will not recover), so the P18.4 driver can drop it. A
    /// looping motion that has not been stopped is never finished.
    #[must_use]
    pub fn is_finished(&self, elapsed: f32, stopped_at: Option<f32>) -> bool {
        match self.ease_out_start(stopped_at) {
            None => false,
            Some(start) => elapsed >= start + self.ease_out_duration,
        }
    }

    /// The elapsed time at which the motion begins easing out, or `None` while it
    /// still holds full weight (a looping motion the simulator has not dropped).
    ///
    /// A non-looping motion eases out so it ends at its duration (stop one
    /// ease-out before the end); an explicit `stopped_at` wins when it is earlier.
    fn ease_out_start(&self, stopped_at: Option<f32>) -> Option<f32> {
        let natural = (!self.loops).then(|| (self.duration - self.ease_out_duration).max(0.0));
        match (stopped_at, natural) {
            (Some(stop), Some(nat)) => Some(stop.min(nat)),
            (Some(stop), None) => Some(stop),
            (None, natural) => natural,
        }
    }

    /// The cubic ease-in weight at `elapsed` seconds (residual 0, fade weight 1):
    /// 1 for a zero ease-in duration, else the smoothstep of `elapsed /
    /// ease_in_duration`.
    fn ease_in_weight(&self, elapsed: f32) -> f32 {
        if self.ease_in_duration <= 0.0 {
            1.0
        } else {
            cubic_step(elapsed / self.ease_in_duration)
        }
    }
}

/// The reference viewer's `cubic_step(x)` (`llmath.h`): the smoothstep
/// `x²·(3 − 2x)` with `x` clamped to `0..=1`, used for the animation ease-in /
/// ease-out weight ramps.
#[must_use]
pub(crate) fn cubic_step(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
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
pub(crate) fn nlerp_quaternions(fraction: f32, a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
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
pub(crate) fn lerp_vector3(fraction: f32, a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
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

    use crate::decode::{HandPose, JointMotion, JointPriority, Motion, PositionKey, RotationKey};

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

    /// A looping motion with the given ease-in / ease-out durations for the
    /// pose-weight tests (a long duration so the natural end never interferes).
    fn easing_motion(loops: bool, ease_in: f32, ease_out: f32) -> Motion {
        Motion {
            base_priority: JointPriority::LOW,
            duration: 30.0,
            emote_name: String::new(),
            loop_in_point: 0.0,
            loop_out_point: 30.0,
            loops,
            ease_in_duration: ease_in,
            ease_out_duration: ease_out,
            hand_pose: HandPose::RELAXED,
            joints: Vec::new(),
            constraints: Vec::new(),
        }
    }

    #[test]
    fn pose_weight_eases_in_cubically() {
        let motion = easing_motion(true, 2.0, 1.0);
        approx(motion.pose_weight(0.0, None), 0.0);
        // cubic_step(0.5) = 0.5.
        approx(motion.pose_weight(1.0, None), 0.5);
        // Fully eased in and holding while signalled.
        approx(motion.pose_weight(2.0, None), 1.0);
        approx(motion.pose_weight(10.0, None), 1.0);
    }

    #[test]
    fn pose_weight_zero_ease_in_is_immediately_full() {
        let motion = easing_motion(true, 0.0, 0.0);
        approx(motion.pose_weight(0.0, None), 1.0);
    }

    #[test]
    fn pose_weight_eases_out_after_stop() {
        let motion = easing_motion(true, 0.0, 2.0);
        // Stopped at t = 5; at the stop it still holds full weight.
        approx(motion.pose_weight(5.0, Some(5.0)), 1.0);
        // Halfway through the ease-out: cubic_step(1 - 0.5) = cubic_step(0.5) = 0.5.
        approx(motion.pose_weight(6.0, Some(5.0)), 0.5);
        // Past the tail: fully faded.
        approx(motion.pose_weight(7.0, Some(5.0)), 0.0);
        approx(motion.pose_weight(9.0, Some(5.0)), 0.0);
    }

    #[test]
    fn pose_weight_stop_mid_ease_in_fades_from_partial() {
        let motion = easing_motion(true, 2.0, 2.0);
        // Stopped at t = 1, where the ease-in weight is cubic_step(0.5) = 0.5.
        // At the stop the weight is that residual, then it eases out from there.
        approx(motion.pose_weight(1.0, Some(1.0)), 0.5);
        // Fully faded once the ease-out tail passes.
        approx(motion.pose_weight(3.0, Some(1.0)), 0.0);
    }

    #[test]
    fn non_looping_motion_eases_out_to_finish_at_duration() {
        // duration 30, ease-out 2: eases out over [28, 30] even without an explicit
        // stop, and is finished at 30.
        let motion = easing_motion(false, 0.0, 2.0);
        approx(motion.pose_weight(27.0, None), 1.0);
        approx(motion.pose_weight(29.0, None), 0.5);
        assert!(!motion.is_finished(29.0, None));
        assert!(motion.is_finished(30.0, None));
        // A looping motion never finishes while signalled.
        let looping = easing_motion(true, 0.0, 2.0);
        assert!(!looping.is_finished(1000.0, None));
    }

    #[test]
    fn is_finished_when_ease_out_tail_passes() {
        let motion = easing_motion(true, 0.0, 2.0);
        assert!(!motion.is_finished(6.0, Some(5.0)));
        assert!(motion.is_finished(7.0, Some(5.0)));
    }

    /// A motion with the given base priority and explicit joint priorities, for the
    /// [`Motion::max_priority`] tests.
    fn priority_motion(base: JointPriority, joints: &[JointPriority]) -> Motion {
        let mut motion = timing_motion(true, 4.0, 0.0, 4.0);
        motion.base_priority = base;
        motion.joints = joints
            .iter()
            .map(|priority| JointMotion {
                name: "mPelvis".to_owned(),
                priority: *priority,
                rotation_keys: Vec::new(),
                position_keys: Vec::new(),
            })
            .collect();
        motion
    }

    #[test]
    fn max_priority_is_the_highest_explicit_joint_priority() {
        let motion = priority_motion(
            JointPriority::LOW,
            &[
                JointPriority::LOW,
                JointPriority::HIGH,
                JointPriority::MEDIUM,
            ],
        );
        assert_eq!(motion.max_priority(), JointPriority::HIGH);
    }

    #[test]
    fn max_priority_ignores_the_base_priority_and_use_motion_joints() {
        // The reference seeds `mMaxPriority` at LOW, *not* at the base priority, and
        // a joint deferring to the motion (USE_MOTION) never raises it — so a
        // HIGHEST-base motion whose joints all defer arbitrates hand poses at LOW.
        let motion = priority_motion(
            JointPriority::HIGHEST,
            &[JointPriority::USE_MOTION, JointPriority::USE_MOTION],
        );
        assert_eq!(motion.max_priority(), JointPriority::LOW);
        // With no joints at all it is still LOW.
        assert_eq!(
            priority_motion(JointPriority::HIGH, &[]).max_priority(),
            JointPriority::LOW
        );
    }

    #[test]
    fn max_priority_of_an_additive_motion_is_its_clamped_base() {
        // A raw base priority of ADDITIVE (7) decodes clamped to 6, and the reference
        // seeds `mMaxPriority` from it in exactly that case — above every named joint
        // priority, so an additive motion's hand pose outranks the others'.
        let base = JointPriority::ADDITIVE_CLAMPED;
        let motion = priority_motion(base, &[JointPriority::HIGHEST]);
        assert_eq!(motion.max_priority(), base);
        assert!(motion.max_priority() > JointPriority::HIGHEST);
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
