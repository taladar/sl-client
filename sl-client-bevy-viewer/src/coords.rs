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
use sl_client_bevy::Vector;

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

#[cfg(test)]
mod tests {
    use super::{sl_to_bevy_rotation, sl_to_bevy_vec};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Vector;

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
}
