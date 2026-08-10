//! The listener: where "the ears" are, and the math that turns a source's world
//! position into the listener-relative offset firewheel's spatial node wants.
//!
//! Whether the ears sit at the camera or at the avatar's head is a
//! reference-viewer preference ([`EarMode`]); the viewer records the choice and
//! feeds the matching pose in. This module is engine-agnostic: positions are
//! plain `[f32; 3]` in whatever right-handed, `Y`-up world frame the caller uses
//! (the Bevy viewer's world frame), and the produced offset is in firewheel's
//! spatial convention (`+x` right of the listener, and the magnitude of the
//! whole vector is the distance).

/// Which pose the listener follows: the camera, or the avatar's head.
///
/// This mirrors the reference viewer's "camera vs. avatar" audio preference. It
/// changes how everything sounds, so it is a user setting rather than a
/// hard-coded choice; the mixer only ever sees the resolved [`Listener`] pose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EarMode {
    /// The ears follow the camera. This is the reference viewer's default and
    /// what most users expect when the camera is orbited away from the avatar.
    #[default]
    Camera,
    /// The ears follow the avatar's head, regardless of where the camera is.
    AvatarHead,
}

/// Normalise a vector, returning `None` if it is (near) zero length.
fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let [x, y, z] = v;
    let len_sq = x * x + y * y + z * z;
    if len_sq <= f32::EPSILON {
        return None;
    }
    let inv = len_sq.sqrt().recip();
    Some([x * inv, y * inv, z * inv])
}

/// The dot product of two vectors.
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The cross product `a × b`.
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The listener pose: position and orientation in the caller's world frame.
///
/// `forward` and `up` need not be perfectly orthonormal — they are
/// re-orthonormalised internally. In a right-handed `Y`-up frame the listener's
/// right is `forward × up`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Listener {
    /// World position of the ears.
    position: [f32; 3],
    /// Unit forward direction (the way the listener faces).
    forward: [f32; 3],
    /// Unit up direction.
    up: [f32; 3],
}

impl Default for Listener {
    /// At the origin, facing `-Z` with `+Y` up — the identity camera pose in a
    /// right-handed `Y`-up frame.
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
        }
    }
}

impl Listener {
    /// Build a listener from a world position and a forward / up orientation.
    ///
    /// Degenerate (zero-length or parallel) `forward` / `up` inputs fall back to
    /// the default orientation so the mixer never produces `NaN` offsets.
    #[must_use]
    pub fn new(position: [f32; 3], forward: [f32; 3], up: [f32; 3]) -> Self {
        let default = Self::default();
        let forward = normalize(forward).unwrap_or(default.forward);
        let up = normalize(up).unwrap_or(default.up);
        Self {
            position,
            forward,
            up,
        }
    }

    /// The listener's world position.
    #[must_use]
    pub const fn position(&self) -> [f32; 3] {
        self.position
    }

    /// The listener's orthonormal basis as `(right, up, forward)` unit vectors.
    ///
    /// `up` is re-derived from `right × forward` so the three are orthonormal
    /// even when the supplied `forward` / `up` were not exactly perpendicular.
    #[must_use]
    fn basis(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let forward = self.forward;
        // Right-handed, Y-up: right = forward × up.
        let right = normalize(cross(forward, self.up)).unwrap_or([1.0, 0.0, 0.0]);
        // Re-orthonormalise up so a slightly-off input does not skew offsets.
        let up = normalize(cross(right, forward)).unwrap_or([0.0, 1.0, 0.0]);
        (right, up, forward)
    }

    /// The offset from the listener to `source_position`, expressed in the
    /// listener's frame in firewheel's spatial convention: `x` is the rightward
    /// component (the pan axis), `y` the up component, `z` the forward
    /// component. The vector's magnitude is the listener→source distance.
    #[must_use]
    pub fn source_offset(&self, source_position: [f32; 3]) -> [f32; 3] {
        let delta = [
            source_position[0] - self.position[0],
            source_position[1] - self.position[1],
            source_position[2] - self.position[2],
        ];
        let (right, up, forward) = self.basis();
        [dot(delta, right), dot(delta, up), dot(delta, forward)]
    }

    /// The straight-line distance from the listener to `source_position`.
    #[must_use]
    pub fn distance_to(&self, source_position: [f32; 3]) -> f32 {
        let delta = [
            source_position[0] - self.position[0],
            source_position[1] - self.position[1],
            source_position[2] - self.position[2],
        ];
        dot(delta, delta).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identity listener (origin, facing `-Z`, `+Y` up) has right `= +X`.
    #[test]
    fn identity_basis_is_right_handed() {
        let l = Listener::default();
        let (right, up, forward) = l.basis();
        assert!((right[0] - 1.0).abs() < 1e-6, "right = +X, got {right:?}");
        assert!((up[1] - 1.0).abs() < 1e-6, "up = +Y, got {up:?}");
        assert!(
            (forward[2] + 1.0).abs() < 1e-6,
            "forward = -Z, got {forward:?}"
        );
    }

    #[test]
    fn source_in_front_maps_to_positive_z() {
        let l = Listener::default();
        // 5m in front (facing -Z) => forward component +5, no pan.
        let off = l.source_offset([0.0, 0.0, -5.0]);
        assert!(off[0].abs() < 1e-6, "no pan, got x={}", off[0]);
        assert!((off[2] - 5.0).abs() < 1e-6, "forward=+5, got z={}", off[2]);
    }

    #[test]
    fn source_to_the_right_pans_positive_x() {
        let l = Listener::default();
        // Facing -Z, the listener's right is +X.
        let off = l.source_offset([3.0, 0.0, 0.0]);
        assert!((off[0] - 3.0).abs() < 1e-6, "right=+3, got x={}", off[0]);
    }

    #[test]
    fn rotated_listener_pans_correctly() {
        // Face +X, up +Y => right = forward × up = (1,0,0)×(0,1,0) = (0,0,1)... check.
        let l = Listener::new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        // A source straight ahead (+X) is forward, no pan.
        let ahead = l.source_offset([4.0, 0.0, 0.0]);
        assert!(ahead[0].abs() < 1e-6, "ahead no pan, got {ahead:?}");
        assert!(
            (ahead[2] - 4.0).abs() < 1e-6,
            "ahead forward=+4, got {ahead:?}"
        );
    }

    #[test]
    fn distance_is_translation_invariant() {
        let l = Listener::new([10.0, 20.0, 30.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
        let d = l.distance_to([10.0, 20.0, 33.0]);
        assert!((d - 3.0).abs() < 1e-6, "distance 3, got {d}");
        let off = l.source_offset([10.0, 20.0, 33.0]);
        let mag = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
        assert!((mag - 3.0).abs() < 1e-6, "offset magnitude = distance");
    }

    #[test]
    fn degenerate_orientation_falls_back() {
        let l = Listener::new([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        // Falls back to identity; must not produce NaN.
        let off = l.source_offset([1.0, 2.0, 3.0]);
        assert!(off.iter().all(|c| c.is_finite()));
    }
}
