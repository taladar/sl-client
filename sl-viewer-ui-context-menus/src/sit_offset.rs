//! The `AgentRequestSit` **offset** the viewer sends when sitting on a prim with
//! **no `llSitTarget`** — a faithful port of the reference's
//! `LLAgentCamera::calcFocusOffset` (`llagentcamera.cpp`), which the reference
//! reuses as the sit offset (`handle_object_sit(object, pick.mObjectOffset)`).
//!
//! # Why the raw pick point is wrong
//!
//! The obvious offset — the ray's surface-hit point — does **not** match the
//! reference. The reference instead projects the click onto the prim's
//! most-camera-facing axial plane **through the prim centre**, clips it to the
//! bounding box, then biases back toward the actual surface point by how close the
//! camera is. The simulator adds this offset **unrotated** to the prim's absolute
//! position (OpenSim `ScenePresence.SendSitResponse`:
//! `pos = part.AbsolutePosition + offset`), so the offset is in **region /
//! world-axis-aligned** Second Life metres, not the prim's local frame — which is
//! the other half of the old mismatch (our surface point was in the prim's local
//! frame, wrong on a rotated prim).
//!
//! On a physics grid (Second Life) the simulator's `SitAvatar` raycast refines the
//! final spot, so the offset is a hint; on a physics-less grid the simulator uses
//! it directly (`pos + offset + ½·avatarHeight`), so matching the reference means
//! matching this computation.
//!
//! # Coordinate space
//!
//! The reference works in agent (region) space. This port does too: every input
//! is a **Second Life** vector (Z-up, region axes) expressed as a [`Vec3`] with
//! the Second Life components packed verbatim (so `q_sl · v` rotates within
//! Second Life space), and the result is a Second Life [`Vector`]. The caller
//! converts the Bevy scene data to this space at the boundary
//! (`object_menu`).
//!
//! # Deliberate simplification
//!
//! The reference's final surface-bias step also nudges by the camera's FOV-zoom
//! factor (alt-zoom). This viewer has no FOV-zoom alt-zoom, so that factor is
//! taken as zero (the virtual camera is the real camera) — the ordinary,
//! not-alt-zoomed case, reproduced exactly.

use bevy::math::{Quat, Vec3};

use sl_client_bevy::Vector;

/// A tiny lower bound on a prim extent, so the axial-plane proportions never
/// divide by zero (the reference's `object_extents.clamp(0.001f, F32_MAX)`).
const MIN_EXTENT: f32 = 0.001;

/// Component-wise vector subtract (`a - b`), avoiding the glam `-` operator the
/// workspace `arithmetic_side_effects` lint trips on (as `camera` does).
#[must_use]
pub const fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise vector add (`a + b`).
const fn vadd(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// Component-wise vector scale (`v * s`).
const fn vscale(v: Vec3, s: f32) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

/// The reference's `LLAgentCamera::calcFocusOffset`, in Second Life space: the sit
/// offset (relative to the seat prim, region-axis-aligned) for a prim with no sit
/// target.
///
/// All vectors are Second Life (Z-up) packed into a [`Vec3`], **relative to the
/// prim** where noted:
/// - `obj_rotation` — the prim's world orientation (a Second Life quaternion).
/// - `extents` — the prim's size per Second Life axis (its scale).
/// - `cam_from_obj` — prim → camera.
/// - `mouse_dir` — the unit click ray direction (camera → cursor).
/// - `hit_from_obj` — prim → the ray's surface-hit point.
/// - `cam_at` — the camera's unit forward (at-axis).
#[must_use]
pub fn sit_focus_offset(
    obj_rotation: Quat,
    extents: Vec3,
    cam_from_obj: Vec3,
    mouse_dir: Vec3,
    hit_from_obj: Vec3,
    cam_at: Vec3,
) -> Vector {
    let extents = extents.max(Vec3::splat(MIN_EXTENT));
    let inv_obj_rot = obj_rotation.inverse();

    // The unit ray from the prim centre toward the camera, in the prim's local
    // frame — its largest component (scaled by the prim's extents) picks the
    // axial plane most facing the camera.
    let obj_to_cam = inv_obj_rot.mul_vec3(cam_from_obj).normalize_or_zero();
    let proportions = Vec3::new(
        (obj_to_cam.x / extents.x).abs(),
        (obj_to_cam.y / extents.y).abs(),
        (obj_to_cam.z / extents.z).abs(),
    );
    // The world-space normal of that plane: the chosen local axis carried to world.
    let plane_normal = if proportions.x > proportions.y && proportions.x > proportions.z {
        obj_rotation.mul_vec3(Vec3::X)
    } else if proportions.y > proportions.z {
        obj_rotation.mul_vec3(Vec3::Y)
    } else {
        obj_rotation.mul_vec3(Vec3::Z)
    }
    .normalize_or_zero();

    // Project the click ray onto that plane through the prim centre (the reference's
    // `mousePointOnPlaneGlobal`). Everything here is relative to the prim, so the
    // plane passes through the origin.
    let denom = mouse_dir.dot(plane_normal);
    let focus_pt = if denom.abs() < 1e-6 {
        // Ray parallel to the plane: fall back to the surface hit.
        hit_from_obj
    } else {
        let t = -cam_from_obj.dot(plane_normal) / denom;
        vadd(cam_from_obj, vscale(mouse_dir, t))
    };

    // The focus offset and the camera→focus vector, in the prim's local frame.
    let mut focus_offset = inv_obj_rot.mul_vec3(focus_pt);
    let camera_to_focus = inv_obj_rot.mul_vec3(vsub(focus_pt, cam_from_obj));

    // Clip the focus offset back inside the bounding box along whichever axis it is
    // *most* outside, pushing along the camera→focus vector (the reference keeps the
    // point under the cursor while pulling it in bounds).
    let half = vscale(extents, 0.5);
    let clip_fraction = clip_fractions(focus_offset, camera_to_focus, half);
    let abs_clip = clip_fraction.abs();
    let clip = if abs_clip.x > abs_clip.y && abs_clip.x > abs_clip.z {
        clip_fraction.x
    } else if abs_clip.y > abs_clip.z {
        clip_fraction.y
    } else {
        clip_fraction.z
    };
    focus_offset = vsub(focus_offset, vscale(camera_to_focus, clip));

    // Back to world space (relative to the prim).
    let focus_offset_world = obj_rotation.mul_vec3(focus_offset);

    // Bias toward the actual surface point when the camera is (relatively) close,
    // toward the planar centre when far — unless the camera is inside the prim's
    // bounding box, where the planar offset is kept. Matches the reference's final
    // block with the FOV-zoom factor taken as zero.
    let surface_rel = hit_from_obj;
    if aabb_contains(cam_from_obj, obj_rotation, half) {
        return pack_sl(focus_offset_world);
    }
    let rel_dist = surface_rel.dot(cam_at).abs();
    let view_dist = vsub(surface_rel, cam_from_obj).length();
    if view_dist < 1e-6 {
        return pack_sl(surface_rel);
    }
    let bias = clamp_rescale(rel_dist / view_dist, 0.1, 0.7, 0.0, 1.0);
    pack_sl(focus_offset_world.lerp(surface_rel, bias))
}

/// The per-axis fraction of `camera_to_focus` needed to pull `focus_offset` back
/// inside the `half`-extent box along each axis (the reference's `clip_fraction`
/// loop): the signed distance outside the box divided by the camera→focus
/// component, or zero when that component is negligible.
fn clip_fractions(focus_offset: Vec3, camera_to_focus: Vec3, half: Vec3) -> Vec3 {
    let dist_out = Vec3::new(
        dist_out_of_bounds(focus_offset.x, half.x),
        dist_out_of_bounds(focus_offset.y, half.y),
        dist_out_of_bounds(focus_offset.z, half.z),
    );
    Vec3::new(
        clip_axis(dist_out.x, camera_to_focus.x),
        clip_axis(dist_out.y, camera_to_focus.y),
        clip_axis(dist_out.z, camera_to_focus.z),
    )
}

/// How far `value` sits outside `[-half, half]`, keeping its sign (zero when in
/// bounds) — the reference's `dist_out_of_bounds`.
fn dist_out_of_bounds(value: f32, half: f32) -> f32 {
    if value > 0.0 {
        (value - half).max(0.0)
    } else {
        (value + half).min(0.0)
    }
}

/// One axis's clip fraction: `dist_out / camera_to_focus`, or zero when the
/// camera→focus component is negligible (avoid dividing by a tiny number).
fn clip_axis(dist_out: f32, camera_to_focus: f32) -> f32 {
    if camera_to_focus.abs() < 0.0001 {
        0.0
    } else {
        dist_out / camera_to_focus
    }
}

/// Whether the point `p` (relative to the prim, world axes) is inside the prim's
/// oriented bounding box — approximated by the axis-aligned box of the oriented
/// half-extents, the reference's `getBoundingBoxAgent().containsPointAgent`.
fn aabb_contains(p: Vec3, obj_rotation: Quat, half: Vec3) -> bool {
    let x = obj_rotation.mul_vec3(Vec3::X);
    let y = obj_rotation.mul_vec3(Vec3::Y);
    let z = obj_rotation.mul_vec3(Vec3::Z);
    // The AABB half-size along each world axis is the sum of the oriented
    // half-extents projected onto that axis.
    let aabb_half = Vec3::new(
        x.x.abs() * half.x + y.x.abs() * half.y + z.x.abs() * half.z,
        x.y.abs() * half.x + y.y.abs() * half.y + z.y.abs() * half.z,
        x.z.abs() * half.x + y.z.abs() * half.y + z.z.abs() * half.z,
    );
    p.x.abs() <= aabb_half.x && p.y.abs() <= aabb_half.y && p.z.abs() <= aabb_half.z
}

/// Clamp `value` into `[from_min, from_max]` and rescale it to `[to_min, to_max]`
/// — the reference's `clamp_rescale` (with `from_min < from_max`).
fn clamp_rescale(value: f32, from_min: f32, from_max: f32, to_min: f32, to_max: f32) -> f32 {
    let clamped = value.clamp(from_min, from_max);
    let span = from_max - from_min;
    if span.abs() < f32::EPSILON {
        return to_min;
    }
    to_min + (to_max - to_min) * (clamped - from_min) / span
}

/// Pack a Second Life-space [`Vec3`] (verbatim components) back into a [`Vector`].
const fn pack_sl(v: Vec3) -> Vector {
    Vector {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

#[cfg(test)]
mod tests {
    use super::sit_focus_offset;
    use bevy::math::{Quat, Vec3};

    /// Looking straight down (camera above) at an axis-aligned box, the click ray
    /// through a point on the top surface projects onto the horizontal plane
    /// **through the box centre** (the most-camera-facing axial plane is the top,
    /// normal +Z), so the offset's Z is the box centre (0), not the top surface —
    /// this is the whole point of the heuristic (the raw surface hit would give the
    /// top). The X/Y stay under the cursor.
    #[test]
    fn top_down_click_projects_to_centre_plane() {
        // A 2×2×2 box at the origin, unrotated.
        let extents = Vec3::splat(2.0);
        // Camera 10 m straight up, looking down.
        let cam_from_obj = Vec3::new(0.0, 0.0, 10.0);
        let cam_at = Vec3::new(0.0, 0.0, -1.0);
        // Click ray hits the top face at (0.5, 0.5, 1.0); direction is down-ish from
        // the camera toward that point.
        let hit_from_obj = Vec3::new(0.5, 0.5, 1.0);
        let mouse_dir = super::vsub(hit_from_obj, cam_from_obj).normalize();

        let offset = sit_focus_offset(
            Quat::IDENTITY,
            extents,
            cam_from_obj,
            mouse_dir,
            hit_from_obj,
            cam_at,
        );
        // The camera is far (10 m) vs the box (2 m), so the surface bias is ~0 and
        // the offset stays on the centre plane: Z ≈ 0, not the top (Z = 1).
        assert!(
            offset.z.abs() < 0.05,
            "top-down click projects to the centre plane (z≈0), got z={}",
            offset.z,
        );
        // And it stays roughly under the cursor in X/Y.
        assert!(
            (offset.x - 0.5).abs() < 0.1 && (offset.y - 0.5).abs() < 0.1,
            "offset stays under the cursor, got ({}, {})",
            offset.x,
            offset.y,
        );
    }

    /// The offset is clipped to the bounding box: a click ray that would project
    /// far outside the box is pulled back within its half-extents.
    #[test]
    fn offset_is_clipped_to_the_bounding_box() {
        let extents = Vec3::splat(2.0); // half-extent 1.0
        let cam_from_obj = Vec3::new(0.0, 0.0, 10.0);
        let cam_at = Vec3::new(0.0, 0.0, -1.0);
        // A hit way off to the side (well outside the box footprint).
        let hit_from_obj = Vec3::new(5.0, 0.0, 1.0);
        let mouse_dir = super::vsub(hit_from_obj, cam_from_obj).normalize();

        let offset = sit_focus_offset(
            Quat::IDENTITY,
            extents,
            cam_from_obj,
            mouse_dir,
            hit_from_obj,
            cam_at,
        );
        assert!(
            offset.x.abs() <= 1.0 + 0.01,
            "the offset x is clipped to the half-extent, got x={}",
            offset.x,
        );
    }
}
