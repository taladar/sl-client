//! The single Second Life ↔ Bevy coordinate-system conversion.
//!
//! Second Life (and OpenSim) use a right-handed, **Z-up** world: `+X` east,
//! `+Y` north, `+Z` up. Bevy is right-handed but **Y-up**: `+X` right, `+Y`
//! up, `-Z` forward. Every geometry crate in this workspace (`sl-prim`,
//! `sl-mesh`, `sl-sculpt`) stays in Second Life space; this conversion is
//! applied once, at the entity [`Transform`](bevy::prelude::Transform) and
//! camera boundary, so nothing downstream has to think about the axis flip.
//!
//! The mapping is the proper rotation `(x, y, z) -> (x, z, -y)` (a `-90°` turn
//! about the shared `X` axis). It has determinant `+1`, so it preserves
//! handedness — a Second Life rotation stays a rotation, and winding order is
//! unchanged. [`sl_to_bevy_vec`] applies it to a position; its quaternion form
//! [`sl_to_bevy_rotation`] applies it as an entity `Transform` rotation, so a
//! mesh whose vertices are kept in Second Life space (as terrain patches and,
//! in Phase 5, object meshes are) lands correctly in Bevy's Y-up world.

use bevy::math::{Quat, Vec3};
use sl_client_bevy::{Rotation, Vector};

/// Convert a Second Life position [`Vector`] (Z-up metres) into a Bevy
/// [`Vec3`] (Y-up).
///
/// Applies the `(x, y, z) -> (x, z, -y)` axis map described in the module
/// documentation.
#[must_use]
pub(crate) fn sl_to_bevy_vec(vector: &Vector) -> Vec3 {
    Vec3::new(vector.x, vector.z, -vector.y)
}

/// The rotation half of the Second Life → Bevy boundary: the quaternion form of
/// the `(x, y, z) -> (x, z, -y)` axis map, a `-90°` turn about the shared `X`
/// axis.
///
/// Applied as an entity [`Transform`](bevy::prelude::Transform) rotation (with
/// its translation set from [`sl_to_bevy_vec`]), it carries a mesh whose
/// vertices — positions *and* normals — are kept in Second Life space (Z-up)
/// into Bevy's Y-up world. Terrain patches build their geometry relative to the
/// patch origin in Second Life space and rely on this to orient it.
#[must_use]
pub(crate) fn sl_to_bevy_rotation() -> Quat {
    Quat::from_rotation_x(-core::f32::consts::FRAC_PI_2)
}

/// A Second Life [`Rotation`] (a unit quaternion in Second Life's Z-up frame) as
/// a Bevy [`Quat`], with the axis components carried across verbatim.
///
/// This does **not** apply the Second Life → Bevy basis change: it is the
/// rotation expressed in Second Life space, for use as the *local* rotation of a
/// linkset child whose parent entity already carries the single
/// [`sl_to_bevy_rotation`] basis change (so the whole subtree stays in Second
/// Life space and is converted once at the root).
#[must_use]
pub(crate) fn sl_rotation_to_quat(rotation: &Rotation) -> Quat {
    let quat = Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.s);
    // The wire always carries a unit quaternion, but guard a degenerate (zero /
    // non-finite) one so a bad object update can never poison a `Transform` with
    // a NaN rotation.
    if quat.length_squared().is_finite() && quat.length_squared() > f32::EPSILON {
        quat.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// A Second Life object's world [`Rotation`] as a Bevy [`Quat`], composing the
/// Second Life → Bevy basis change with the object's own orientation.
///
/// This is the rotation half of a *root* object's world `Transform` (its
/// translation coming from [`sl_to_bevy_vec`]): it maps the object's Second Life
/// local space directly into Bevy's Y-up world, the same way
/// [`sl_to_bevy_rotation`] orients a terrain patch. Linkset children instead use
/// the un-changed [`sl_rotation_to_quat`], relying on the parent to carry the
/// basis change.
#[must_use]
pub(crate) fn sl_to_bevy_object_rotation(rotation: &Rotation) -> Quat {
    // `Quat::mul_quat` rather than the `*` operator to stay clear of the
    // workspace `arithmetic_side_effects` lint.
    sl_to_bevy_rotation().mul_quat(sl_rotation_to_quat(rotation))
}

#[cfg(test)]
mod tests {
    use super::{
        sl_rotation_to_quat, sl_to_bevy_object_rotation, sl_to_bevy_rotation, sl_to_bevy_vec,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Rotation, Vector};

    /// The east/north/up Second Life axes map to Bevy right/forward-negation/up.
    #[test]
    fn axes_map_z_up_to_y_up() {
        // Second Life `+Z` (up) becomes Bevy `+Y` (up).
        let up = sl_to_bevy_vec(&Vector {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        });
        assert_eq!(up, bevy::math::Vec3::new(0.0, 1.0, 0.0));
        // Second Life `+Y` (north) becomes Bevy `-Z`.
        let north = sl_to_bevy_vec(&Vector {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        });
        assert_eq!(north, bevy::math::Vec3::new(0.0, 0.0, -1.0));
        // Second Life `+X` (east) stays Bevy `+X`.
        let east = sl_to_bevy_vec(&Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        });
        assert_eq!(east, bevy::math::Vec3::new(1.0, 0.0, 0.0));
    }

    /// The rotation quaternion reproduces the position axis map on any vector,
    /// so a mesh kept in Second Life space lands where its converted origin
    /// says it should.
    #[test]
    fn rotation_matches_the_position_map() {
        let rotation = sl_to_bevy_rotation();
        for vector in [
            Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            Vector {
                x: -4.0,
                y: 5.0,
                z: -6.0,
            },
        ] {
            let rotated = rotation * bevy::math::Vec3::new(vector.x, vector.y, vector.z);
            let mapped = sl_to_bevy_vec(&vector);
            assert!(
                rotated.abs_diff_eq(mapped, 1.0e-5),
                "rotation {rotated:?} should match axis map {mapped:?}"
            );
        }
    }

    /// The identity Second Life rotation maps (as an object's local rotation) to
    /// the Bevy identity, and (as a world rotation) to the plain basis change.
    #[test]
    fn identity_rotation_is_identity() {
        let identity = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };
        assert!(sl_rotation_to_quat(&identity).abs_diff_eq(bevy::math::Quat::IDENTITY, 1.0e-6));
        assert!(sl_to_bevy_object_rotation(&identity).abs_diff_eq(sl_to_bevy_rotation(), 1.0e-6));
    }

    /// A degenerate (zero) rotation is guarded to the identity rather than
    /// poisoning a `Transform` with a NaN.
    #[test]
    fn degenerate_rotation_falls_back_to_identity() {
        let zero = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 0.0,
        };
        assert_eq!(sl_rotation_to_quat(&zero), bevy::math::Quat::IDENTITY);
    }

    /// A root object's world rotation composes the basis change with the
    /// object's own orientation, so a mesh point kept in Second Life local space
    /// lands where applying the object rotation then the axis map would put it.
    #[test]
    fn object_rotation_composes_with_the_basis_change() {
        // A 90° yaw about Second Life +Z (up): east (+X) turns to north (+Y).
        let half = core::f32::consts::FRAC_1_SQRT_2;
        let yaw = Rotation {
            x: 0.0,
            y: 0.0,
            z: half,
            s: half,
        };
        let world = sl_to_bevy_object_rotation(&yaw);
        // The object's local +X (Second Life east) should end up at Bevy -Z
        // (Second Life north, after the Z-up → Y-up map).
        let mapped = world * bevy::math::Vec3::X;
        assert!(
            mapped.abs_diff_eq(bevy::math::Vec3::new(0.0, 0.0, -1.0), 1.0e-5),
            "object +X mapped to {mapped:?}"
        );
    }
}
