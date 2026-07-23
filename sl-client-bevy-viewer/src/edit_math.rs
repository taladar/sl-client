//! Pure geometry for the object-editing tools (`viewer-transform-gizmos`):
//! mouse-ray projections, grid / angle snapping, scale clamps, and the Second
//! Life Euler-angle convention the build floater's rotation fields use.
//!
//! Everything here is **Second Life space** (right-handed Z-up, the frame the
//! wire values live in) expressed in glam [`Vec3`] / [`Quat`]; the callers
//! convert to and from Bevy's Y-up world at the entity boundary exactly as
//! [`crate::coords`] prescribes. No ECS, no I/O — the module is unit-tested
//! math, mirroring the reference viewer's `llmanip` drag geometry:
//!
//! - a **translate arrow** drag intersects the mouse ray with the plane that
//!   contains the drag axis and faces the camera
//!   ([`manip_plane_normal`]), then projects the in-plane motion onto the axis
//!   ([`project_onto_axis`]) — `LLManipTranslate::handleHover`;
//! - a **translate plane** drag uses the in-plane motion directly;
//! - a **rotate ring** drag measures the angle of the cursor about the ring's
//!   axis ([`ring_angle`]) — `LLManipRotate::dragConstrained`;
//! - a **scale handle** drag slides along the handle's line
//!   ([`closest_line_param`]) — `LLManipScale::dragFace` / `dragCorner` via
//!   `nearestPointOnLineFromMouse`.

use bevy::math::{Quat, Vec2, Vec3};
use sl_client_bevy::Rotation;

/// The smallest per-axis prim size an edit will request, in metres — OpenSim's
/// `OS_MIN_PRIM_SCALE` (the most permissive grid; Second Life's own floor is
/// 0.01 m and its simulator clamps whatever it receives, so requesting the
/// permissive bound is safe on both).
pub(crate) const MIN_PRIM_SCALE: f32 = 0.001;

/// The largest per-axis prim size an edit will request, in metres — OpenSim's
/// `OS_DEFAULT_MAX_PRIM_SCALE` (Second Life's tighter 64 m cap is enforced by
/// its simulator).
pub(crate) const MAX_PRIM_SCALE: f32 = 256.0;

/// The rotate manipulator's snap increment, in degrees — the reference's
/// `SNAP_ANGLE_INCREMENT` (360° / 64).
pub(crate) const SNAP_ANGLE_DEG: f32 = 5.625;

/// Component-wise vector addition, avoiding the glam `+` operator the
/// workspace `arithmetic_side_effects` lint trips on.
pub(crate) fn vadd(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// Component-wise vector subtraction (`a - b`).
pub(crate) fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise vector scaling (`v * s`).
pub(crate) fn vscale(v: Vec3, s: f32) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

/// A Second Life wire [`Rotation`] as a glam [`Quat`] in the same (Z-up) frame
/// — no basis change, with a degenerate value guarded to the identity.
pub(crate) fn rotation_to_quat(rotation: &Rotation) -> Quat {
    let quat = Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.s);
    if quat.length_squared().is_finite() && quat.length_squared() > f32::EPSILON {
        quat.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// A glam [`Quat`] (in Second Life's Z-up frame) as the wire [`Rotation`] an
/// object update carries.
pub(crate) fn quat_to_rotation(quat: Quat) -> Rotation {
    let quat = quat.normalize();
    Rotation {
        x: quat.x,
        y: quat.y,
        z: quat.z,
        s: quat.w,
    }
}

/// Intersect the ray `origin + t * dir` with the plane through `plane_point`
/// with normal `plane_normal`, returning the intersection point — or `None`
/// when the ray is (near-)parallel to the plane or the hit is behind the ray
/// origin. The reference's `getMousePointOnPlaneGlobal`.
pub(crate) fn ray_plane_intersect(
    origin: Vec3,
    dir: Vec3,
    plane_point: Vec3,
    plane_normal: Vec3,
) -> Option<Vec3> {
    let denom = dir.dot(plane_normal);
    if denom.abs() < 1.0e-6 {
        return None;
    }
    let t = vsub(plane_point, origin).dot(plane_normal) / denom;
    if !t.is_finite() || t < 0.0 {
        return None;
    }
    Some(vadd(origin, vscale(dir, t)))
}

/// The normal of the drag plane for an **axis** drag: the plane that contains
/// `axis` and faces the camera (looking along `camera_forward`), so the mouse
/// ray always strikes it at a stable angle — the reference's
/// `LLManip::getManipNormal` (`cross = axis × at; normal = cross × axis`).
/// `None` when the axis is (near-)parallel to the view direction, where no such
/// plane is stable.
pub(crate) fn manip_plane_normal(axis: Vec3, camera_forward: Vec3) -> Option<Vec3> {
    let cross = axis.cross(camera_forward);
    if cross.length_squared() < 1.0e-9 {
        return None;
    }
    Some(cross.cross(axis).normalize())
}

/// The signed distance the in-plane cursor motion `delta` moves along the unit
/// `axis` — the axis-constrained half of a translate-arrow drag.
pub(crate) fn project_onto_axis(delta: Vec3, axis: Vec3) -> f32 {
    delta.dot(axis)
}

/// Snap `value` to the nearest multiple of `grid` (a no-op for a degenerate
/// grid), the translate / scale grid quantisation.
pub(crate) fn snap_to_grid(value: f32, grid: f32) -> f32 {
    if grid <= 1.0e-6 || !grid.is_finite() {
        return value;
    }
    (value / grid).round() * grid
}

/// The angle of `point_minus_center` about a ring whose in-plane orthonormal
/// frame is (`axis_a`, `axis_b`), in radians in `(-π, π]` — the reference's
/// `atan2` cursor angle in `LLManipRotate::dragConstrained`. Measured from
/// `axis_a` towards `axis_b`.
pub(crate) fn ring_angle(point_minus_center: Vec3, axis_a: Vec3, axis_b: Vec3) -> f32 {
    point_minus_center
        .dot(axis_b)
        .atan2(point_minus_center.dot(axis_a))
}

/// Wrap an angle (radians) into `(-π, π]` — the per-frame **step** wrap that
/// keeps an accumulated ring-drag angle continuous across the atan2 seam
/// (without it the delta jumps by a full turn when the cursor crosses ±180°
/// from the grab point).
pub(crate) fn wrap_angle(angle: f32) -> f32 {
    let wrapped =
        (angle + core::f32::consts::PI).rem_euclid(core::f32::consts::TAU) - core::f32::consts::PI;
    if wrapped <= -core::f32::consts::PI {
        wrapped + core::f32::consts::TAU
    } else {
        wrapped
    }
}

/// The **twist** of `quat` about the unit `axis`, in radians in `(-π, π]` —
/// the swing-twist decomposition's angle about the axis (by vector-part
/// projection). Left-composing a rotation of `δ` about `axis` adds `δ` to the
/// twist, which is what lets the rotate gizmo snap an object's **absolute**
/// orientation about the ring axis to repeatable detents instead of a
/// grab-relative delta.
pub(crate) fn twist_about_axis(quat: Quat, axis: Vec3) -> f32 {
    let quat = quat.normalize();
    let projected = Vec3::new(quat.x, quat.y, quat.z).dot(axis);
    2.0 * projected.atan2(quat.w)
}

/// The object-local axis nearest to `local_dir` (a direction expressed in the
/// object's own frame): its index (0 / 1 / 2), the direction's sign on it,
/// and the alignment (the absolute component) — the reference's
/// `nearestAxis` fold of a grid-frame stretch onto **one local scale axis**
/// (`LLManipScale::stretchFace`): the world drag delta divides by the
/// alignment so the object's world extent along the drag grows by the dragged
/// amount.
pub(crate) fn nearest_local_axis(local_dir: Vec3) -> (usize, f32, f32) {
    let abs = local_dir.abs();
    if abs.x >= abs.y && abs.x >= abs.z {
        (0, local_dir.x.signum(), abs.x)
    } else if abs.y >= abs.z {
        (1, local_dir.y.signum(), abs.y)
    } else {
        (2, local_dir.z.signum(), abs.z)
    }
}

/// Snap an angle (radians) to the nearest multiple of `increment` (radians), a
/// no-op for a degenerate increment — the reference's 5.625° rotation detents.
pub(crate) fn snap_angle(angle: f32, increment: f32) -> f32 {
    if increment <= 1.0e-9 || !increment.is_finite() {
        return angle;
    }
    (angle / increment).round() * increment
}

/// The parameter `t` of the point on the line `line_origin + t * line_dir`
/// nearest to the ray `ray_origin + s * ray_dir` (`s` unconstrained), or `None`
/// when the two are (near-)parallel — the reference's
/// `nearestPointOnLineFromMouse`, which the scale handles slide along.
pub(crate) fn closest_line_param(
    line_origin: Vec3,
    line_dir: Vec3,
    ray_origin: Vec3,
    ray_dir: Vec3,
) -> Option<f32> {
    // Solve the two-line closest-point system: with d1 = line_dir, d2 = ray_dir,
    // r = line_origin - ray_origin: t = (b*e - c*d) / (a*c - b*b).
    let a = line_dir.dot(line_dir);
    let b = line_dir.dot(ray_dir);
    let c = ray_dir.dot(ray_dir);
    let r = vsub(line_origin, ray_origin);
    let d = line_dir.dot(r);
    let e = ray_dir.dot(r);
    #[expect(
        clippy::suspicious_operation_groupings,
        reason = "the closest-point denominator really is a*c - b², not a*c - a*b"
    )]
    let denom = a * c - b * b;
    if denom.abs() < 1.0e-9 {
        return None;
    }
    let t = (b * e - c * d) / denom;
    t.is_finite().then_some(t)
}

/// Clamp one prim-scale component to the grid-legal range
/// ([`MIN_PRIM_SCALE`], [`MAX_PRIM_SCALE`]).
pub(crate) const fn clamp_scale(value: f32) -> f32 {
    value.clamp(MIN_PRIM_SCALE, MAX_PRIM_SCALE)
}

/// The world-units-per-`target_px`-pixels factor that keeps a gizmo a constant
/// on-screen size: at `distance` from a perspective camera with vertical field
/// of view `fov_y` (radians) rendering `viewport_height` pixels, an object
/// scaled by the returned factor spans about `target_px` pixels.
pub(crate) fn constant_screen_scale(
    distance: f32,
    fov_y: f32,
    viewport_height: f32,
    target_px: f32,
) -> f32 {
    if viewport_height <= 0.0 {
        return 1.0;
    }
    let world_per_pixel = 2.0 * distance.max(0.01) * (fov_y * 0.5).tan() / viewport_height;
    (world_per_pixel * target_px).max(1.0e-4)
}

/// A Second Life Euler rotation (degrees about X, Y, Z — roll, pitch, yaw) as
/// a wire [`Rotation`], the reference viewer's `LLQuaternion::setQuat(roll,
/// pitch, yaw)` composition (the same formula as
/// [`crate::coords::sl_euler_deg_to_quat`], landing on the wire type).
pub(crate) fn euler_deg_to_rotation(euler_deg: [f32; 3]) -> Rotation {
    let roll = euler_deg[0].to_radians() * 0.5;
    let pitch = euler_deg[1].to_radians() * 0.5;
    let yaw = euler_deg[2].to_radians() * 0.5;
    let (sx, cx) = roll.sin_cos();
    let (sy, cy) = pitch.sin_cos();
    let (sz, cz) = yaw.sin_cos();
    Rotation {
        x: sx * cy * cz + cx * sy * sz,
        y: cx * sy * cz - sx * cy * sz,
        z: cx * cy * sz + sx * sy * cz,
        s: cx * cy * cz - sx * sy * sz,
    }
}

/// A wire [`Rotation`] as Second Life Euler angles (degrees about X, Y, Z —
/// roll, pitch, yaw), the inverse of [`euler_deg_to_rotation`] — the
/// reference's `LLQuaternion::getEulerAngles`, which the build floater's
/// rotation fields display.
///
/// At the pitch singularity (±90°) the roll is folded into the yaw (gimbal
/// lock), matching the reference's behaviour of returning *a* consistent
/// triple rather than failing.
pub(crate) fn rotation_to_euler_deg(rotation: &Rotation) -> [f32; 3] {
    let quat = rotation_to_quat(rotation);
    let (x, y, z, w) = (quat.x, quat.y, quat.z, quat.w);
    // `euler_deg_to_rotation` composes `q = qx(roll) · qy(pitch) · qz(yaw)`
    // (glam convention: the yaw is applied first), i.e. the rotation matrix
    // `R = Rx · Ry · Rz` — so the extraction reads `sin(pitch)` off `m02 =
    // 2(xz + wy)`, the roll off `-m12 / m22`, and the yaw off `-m01 / m00`.
    let sin_pitch = 2.0 * (x * z + w * y);
    if sin_pitch.abs() >= 0.999_999 {
        // Gimbal lock: pitch is ±90°, roll and yaw are no longer independent;
        // put the whole twist in yaw (`m10` / `m11` with roll fixed at zero).
        let pitch = core::f32::consts::FRAC_PI_2.copysign(sin_pitch);
        let yaw = (2.0 * (x * y + w * z)).atan2(1.0 - 2.0 * (x * x + z * z));
        return [0.0, pitch.to_degrees(), yaw.to_degrees()];
    }
    let roll = (2.0 * (w * x - y * z)).atan2(1.0 - 2.0 * (x * x + y * y));
    let pitch = sin_pitch.asin();
    let yaw = (2.0 * (w * z - x * y)).atan2(1.0 - 2.0 * (y * y + z * z));
    [roll.to_degrees(), pitch.to_degrees(), yaw.to_degrees()]
}

/// The screen-space rectangle spanned by two drag corners, as `(min, max)`.
pub(crate) fn rect_from_corners(a: Vec2, b: Vec2) -> (Vec2, Vec2) {
    (a.min(b), a.max(b))
}

/// Whether the screen-space bounding box of `points` (an object's projected
/// bound corners) is selected by the rubber-band rectangle `(min, max)`:
/// **inclusive** (the reference's default `RectSelectInclusive`) selects on any
/// overlap; exclusive requires the object's whole bound inside the rectangle.
/// An empty `points` (nothing projectable — behind the camera) never selects.
pub(crate) fn rect_selects<I>(min: Vec2, max: Vec2, points: I, inclusive: bool) -> bool
where
    I: IntoIterator<Item = Vec2>,
{
    let mut lo = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut hi = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut any = false;
    for point in points {
        any = true;
        lo = lo.min(point);
        hi = hi.max(point);
    }
    if !any {
        return false;
    }
    if inclusive {
        lo.x <= max.x && hi.x >= min.x && lo.y <= max.y && hi.y >= min.y
    } else {
        lo.x >= min.x && hi.x <= max.x && lo.y >= min.y && hi.y <= max.y
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PRIM_SCALE, MIN_PRIM_SCALE, clamp_scale, closest_line_param, constant_screen_scale,
        euler_deg_to_rotation, manip_plane_normal, project_onto_axis, quat_to_rotation,
        ray_plane_intersect, rect_from_corners, rect_selects, ring_angle, rotation_to_euler_deg,
        rotation_to_quat, snap_angle, snap_to_grid, vadd, vscale, vsub,
    };
    use bevy::math::{Quat, Vec2, Vec3};
    use pretty_assertions::assert_eq;

    /// The component-wise helpers do plain arithmetic.
    #[test]
    fn vector_helpers() {
        assert_eq!(
            vadd(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.5, -2.0, 1.0)),
            Vec3::new(1.5, 0.0, 4.0)
        );
        assert_eq!(
            vsub(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.5, 2.0, 1.0)),
            Vec3::new(0.5, 0.0, 2.0)
        );
        assert_eq!(
            vscale(Vec3::new(1.0, -2.0, 3.0), 2.0),
            Vec3::new(2.0, -4.0, 6.0)
        );
    }

    /// A ray straight down onto the ground plane lands where expected; a ray
    /// parallel to the plane, or pointing away from it, yields nothing.
    #[test]
    fn ray_plane_basics() {
        let hit = ray_plane_intersect(
            Vec3::new(1.0, 2.0, 10.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::ZERO,
            Vec3::Z,
        );
        assert_eq!(hit, Some(Vec3::new(1.0, 2.0, 0.0)));
        // Parallel.
        assert_eq!(
            ray_plane_intersect(Vec3::Z, Vec3::X, Vec3::ZERO, Vec3::Z),
            None
        );
        // Behind the origin.
        assert_eq!(
            ray_plane_intersect(Vec3::new(0.0, 0.0, 10.0), Vec3::Z, Vec3::ZERO, Vec3::Z),
            None
        );
    }

    /// The manip plane contains the axis and faces the camera; a view straight
    /// down the axis has no stable plane.
    #[test]
    fn manip_plane_faces_the_camera() {
        // Dragging the X axis while looking along -Y: the plane should be the
        // XZ-ish plane whose normal is ±Y (perpendicular to the axis, facing
        // the camera).
        let normal = manip_plane_normal(Vec3::X, Vec3::new(0.0, -1.0, 0.0)).unwrap_or(Vec3::ZERO);
        assert!(normal != Vec3::ZERO, "oblique view has a plane");
        assert!(normal.dot(Vec3::X).abs() < 1.0e-6, "contains the axis");
        assert!(normal.dot(Vec3::Y).abs() > 0.99, "faces the camera");
        assert_eq!(manip_plane_normal(Vec3::X, Vec3::X), None);
    }

    /// Axis projection is a plain dot product.
    #[test]
    fn axis_projection() {
        let delta = Vec3::new(3.0, 4.0, 0.0);
        assert!((project_onto_axis(delta, Vec3::X) - 3.0).abs() < 1.0e-6);
        assert!((project_onto_axis(delta, Vec3::Y) - 4.0).abs() < 1.0e-6);
    }

    /// Grid snapping rounds to the nearest multiple and passes degenerate grids
    /// through.
    #[test]
    fn grid_snapping() {
        assert!((snap_to_grid(1.26, 0.5) - 1.5).abs() < 1.0e-6);
        assert!((snap_to_grid(1.24, 0.5) - 1.0).abs() < 1.0e-6);
        assert!((snap_to_grid(-0.6, 0.5) + 0.5).abs() < 1.0e-6);
        assert!((snap_to_grid(1.26, 0.0) - 1.26).abs() < 1.0e-6);
    }

    /// The ring angle sweeps from `axis_a` towards `axis_b`.
    #[test]
    fn ring_angle_sweeps_a_to_b() {
        let a = Vec3::X;
        let b = Vec3::Y;
        assert!(ring_angle(Vec3::X, a, b).abs() < 1.0e-6);
        assert!((ring_angle(Vec3::Y, a, b) - core::f32::consts::FRAC_PI_2).abs() < 1.0e-6);
        assert!(
            (ring_angle(Vec3::new(-1.0, 0.0, 0.0), a, b).abs() - core::f32::consts::PI).abs()
                < 1.0e-5
        );
    }

    /// Step wrapping folds any angle into `(-π, π]`, so accumulated ring
    /// deltas stay continuous across the atan2 seam.
    #[test]
    fn angle_wrapping() {
        use core::f32::consts::PI;
        assert!((super::wrap_angle(0.3) - 0.3).abs() < 1.0e-6);
        // Just past +π folds to just past -π (a small negative step, not a
        // full-turn jump).
        assert!((super::wrap_angle(PI + 0.1) - (0.1 - PI)).abs() < 1.0e-5);
        assert!((super::wrap_angle(-PI - 0.1) - (PI - 0.1)).abs() < 1.0e-5);
        // Multi-turn values fold too.
        assert!((super::wrap_angle(2.0 * PI + 0.2) - 0.2).abs() < 1.0e-5);
    }

    /// The twist about an axis reads back a pure rotation's angle, and
    /// left-composing a rotation about the axis adds to it — even with a
    /// swing (off-axis rotation) mixed in. This is what makes absolute-twist
    /// snapping repeatable.
    #[test]
    fn twist_reads_and_accumulates() {
        use super::{twist_about_axis, wrap_angle};
        // A pure twist reads back exactly.
        let pure = Quat::from_rotation_z(0.7);
        assert!((twist_about_axis(pure, Vec3::Z) - 0.7).abs() < 1.0e-5);
        // A swing alone has no twist.
        let swing = Quat::from_rotation_x(1.2);
        assert!(twist_about_axis(swing, Vec3::Z).abs() < 1.0e-5);
        // Left-composing a rotation about the axis adds its angle to the
        // twist, swing or not.
        for (base, delta) in [
            (swing, 0.9_f32),
            (
                Quat::from_rotation_x(0.4).mul_quat(Quat::from_rotation_z(0.5)),
                -0.6,
            ),
            (
                Quat::from_rotation_y(0.8).mul_quat(Quat::from_rotation_z(-1.1)),
                2.0,
            ),
        ] {
            let before = twist_about_axis(base, Vec3::Z);
            let after = twist_about_axis(Quat::from_rotation_z(delta).mul_quat(base), Vec3::Z);
            let step = wrap_angle(after - before);
            assert!(
                (step - wrap_angle(delta)).abs() < 1.0e-4,
                "twist step {step} should be {delta}"
            );
        }
    }

    /// The nearest-local-axis fold picks the dominant component with its sign
    /// and alignment — a world-frame stretch of a rotated object lands on one
    /// local scale axis, the reference's `stretchFace` behaviour.
    #[test]
    fn nearest_axis_folds_directions() {
        use super::nearest_local_axis;
        let (index, sign, alignment) = nearest_local_axis(Vec3::new(0.9, 0.1, -0.2));
        assert_eq!((index, sign), (0, 1.0));
        assert!((alignment - 0.9).abs() < 1.0e-6);
        let (index, sign, _alignment) = nearest_local_axis(Vec3::new(0.1, -0.8, 0.3));
        assert_eq!((index, sign), (1, -1.0));
        // A 45° tie picks deterministically (x wins ties with y, y with z).
        let (index, _sign, alignment) = nearest_local_axis(Vec3::new(0.707, 0.707, 0.0));
        assert_eq!(index, 0);
        assert!((alignment - 0.707).abs() < 1.0e-6);
    }

    /// Angle snapping quantises to the reference's 5.625° detents.
    #[test]
    fn angle_snapping() {
        let inc = super::SNAP_ANGLE_DEG.to_radians();
        let snapped = snap_angle(0.12, inc);
        assert!(
            (snapped - inc).abs() < 1.0e-6,
            "0.12 rad snaps to one detent"
        );
        assert!(
            (snap_angle(0.02, inc)).abs() < 1.0e-6,
            "0.02 rad snaps to zero"
        );
        assert!(
            (snap_angle(0.3, 0.0) - 0.3).abs() < 1.0e-6,
            "degenerate passes through"
        );
    }

    /// The closest-line parameter finds where a scale handle sits under the
    /// mouse ray, and parallel lines yield nothing.
    #[test]
    fn line_param_under_a_ray() {
        // Line along X from the origin; ray straight down through (2, 0, z).
        // `unwrap_or(NaN)`: a missing closest point fails the assertion below.
        let t = closest_line_param(
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(2.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -1.0),
        )
        .unwrap_or(f32::NAN);
        assert!((t - 2.0).abs() < 1.0e-5, "closest point at t = 2, got {t}");
        assert_eq!(
            closest_line_param(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X),
            None
        );
    }

    /// Scale clamps stay inside the permissive grid range.
    #[test]
    fn scale_clamps() {
        assert!((clamp_scale(0.0) - MIN_PRIM_SCALE).abs() < 1.0e-9);
        assert!((clamp_scale(1_000.0) - MAX_PRIM_SCALE).abs() < 1.0e-9);
        assert!((clamp_scale(2.5) - 2.5).abs() < 1.0e-9);
    }

    /// The constant-screen-size factor grows linearly with distance.
    #[test]
    fn screen_scale_tracks_distance() {
        let fov = core::f32::consts::FRAC_PI_2;
        let near = constant_screen_scale(5.0, fov, 1000.0, 100.0);
        let far = constant_screen_scale(10.0, fov, 1000.0, 100.0);
        assert!((far / near - 2.0).abs() < 1.0e-3);
    }

    /// Wire ↔ glam quaternion conversions round-trip, and a degenerate wire
    /// rotation guards to the identity.
    #[test]
    fn rotation_quat_roundtrip() {
        let rotation = quat_to_rotation(Quat::from_rotation_z(1.0));
        let quat = rotation_to_quat(&rotation);
        assert!(quat.abs_diff_eq(Quat::from_rotation_z(1.0), 1.0e-6));
        let zero = sl_client_bevy::Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 0.0,
        };
        assert_eq!(rotation_to_quat(&zero), Quat::IDENTITY);
    }

    /// Euler → rotation matches the coords module's reference-faithful
    /// composition, and rotation → Euler inverts it across the range (up to
    /// angle wrapping).
    #[test]
    fn euler_roundtrip() {
        for angles in [
            [0.0, 0.0, 0.0],
            [30.0, 0.0, 0.0],
            [0.0, 45.0, 0.0],
            [0.0, 0.0, 60.0],
            [10.0, 20.0, 30.0],
            [-40.0, 15.0, -120.0],
        ] {
            let rotation = euler_deg_to_rotation(angles);
            let back = rotation_to_euler_deg(&rotation);
            let again = euler_deg_to_rotation(back);
            // Compare via the quaternions (Euler triples are not unique).
            let a = rotation_to_quat(&rotation);
            let b = rotation_to_quat(&again);
            assert!(
                a.abs_diff_eq(b, 1.0e-4) || a.abs_diff_eq(vneg(b), 1.0e-4),
                "{angles:?} → {back:?} should be the same rotation"
            );
        }
    }

    /// Negate a quaternion (the same rotation, opposite sign) for the
    /// double-cover comparison above.
    fn vneg(q: Quat) -> Quat {
        Quat::from_xyzw(-q.x, -q.y, -q.z, -q.w)
    }

    /// The Euler composition here agrees with `coords::sl_euler_deg_to_quat`.
    #[test]
    fn euler_matches_coords_convention() {
        for angles in [
            [90.0, 0.0, 0.0],
            [0.0, 90.0, 0.0],
            [0.0, 0.0, 90.0],
            [10.0, 20.0, 30.0],
        ] {
            let here = rotation_to_quat(&euler_deg_to_rotation(angles));
            let coords = crate::coords::sl_euler_deg_to_quat(angles);
            assert!(
                here.abs_diff_eq(coords, 1.0e-5),
                "{angles:?}: {here:?} vs {coords:?}"
            );
        }
    }

    /// The rubber-band tests: inclusive selects on overlap, exclusive only on
    /// containment, and nothing projectable never selects.
    #[test]
    fn rubber_band_rect() {
        let (min, max) = rect_from_corners(Vec2::new(100.0, 100.0), Vec2::new(10.0, 20.0));
        assert_eq!(min, Vec2::new(10.0, 20.0));
        assert_eq!(max, Vec2::new(100.0, 100.0));
        // An object half in the rectangle.
        let overlapping = [Vec2::new(90.0, 50.0), Vec2::new(150.0, 80.0)];
        assert!(rect_selects(min, max, overlapping, true));
        assert!(!rect_selects(min, max, overlapping, false));
        // Fully inside.
        let inside = [Vec2::new(30.0, 40.0), Vec2::new(50.0, 60.0)];
        assert!(rect_selects(min, max, inside, false));
        // Fully outside.
        let outside = [Vec2::new(200.0, 200.0), Vec2::new(250.0, 240.0)];
        assert!(!rect_selects(min, max, outside, true));
        // Nothing projectable.
        assert!(!rect_selects(min, max, core::iter::empty::<Vec2>(), true));
    }
}
