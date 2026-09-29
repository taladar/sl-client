use bevy::math::Affine3A;
use bevy::prelude::*;
use pretty_assertions::assert_eq;

use super::{Candidates, candidates};

/// The viewport of the pinhole camera, logical pixels.
const VIEWPORT: Vec2 = Vec2::new(800.0, 600.0);

/// A pinhole camera at the origin looking down −z, 100 px per unit at unit
/// depth, centred on the viewport; nothing behind it projects.
fn pinhole(point: Vec3) -> Option<Vec2> {
    (point.z < 0.0).then(|| {
        Vec2::new(
            400.0 + 100.0 * point.x / -point.z,
            300.0 - 100.0 * point.y / -point.z,
        )
    })
}

/// A box of side `size` centred at `centre`, unrotated.
fn cube(centre: Vec3, size: f32) -> Affine3A {
    Affine3A::from_scale_rotation_translation(Vec3::splat(size), Quat::IDENTITY, centre)
}

#[test]
fn a_box_ahead_is_aimed_at_its_centre_first_then_the_face_towards_the_camera() {
    let Candidates {
        bounds,
        on_screen,
        points,
    } = candidates(
        &cube(Vec3::new(0.0, 0.0, -10.0), 2.0),
        Vec3::ZERO,
        VIEWPORT,
        pinhole,
    );
    assert!(on_screen);
    assert_eq!(
        points.first().copied(),
        Some(Vec2::new(400.0, 300.0)),
        "the projected centre leads"
    );
    // Only the near face (+z) faces the camera; its centre is the projected
    // centre again and is dropped as a duplicate, leaving its eight lattice
    // points.
    assert_eq!(points.len(), 9, "{points:?}");
    let near_face = Rect::from_corners(Vec2::new(400.0 - 100.0 / 9.0, 300.0 - 100.0 / 9.0), {
        Vec2::new(400.0 + 100.0 / 9.0, 300.0 + 100.0 / 9.0)
    });
    assert!(
        points.iter().all(|point| near_face.contains(*point)),
        "every point lies on the near face: {points:?}"
    );
    // The far face is 11 units away and smaller, the near one 9 and larger:
    // the bounds are the near face's.
    let bounds = bounds.map(|bounds| {
        (
            bounds.min.x.round(),
            bounds.min.y.round(),
            bounds.max.x.round(),
            bounds.max.y.round(),
        )
    });
    assert_eq!(bounds, Some((389.0, 289.0, 411.0, 311.0)));
}

#[test]
fn a_box_seen_across_its_corner_shows_three_faces() {
    // Up, right and ahead: the camera sees the −x, −y and +z faces.
    let seen = candidates(
        &cube(Vec3::new(3.0, 3.0, -10.0), 2.0),
        Vec3::ZERO,
        VIEWPORT,
        pinhole,
    );
    // The centre, three face centres, and 8 lattice points on each face less
    // the ones that project within two pixels of another.
    assert!(seen.points.len() > 20, "{:?}", seen.points);
    assert!(seen.points.len() <= super::MAX_CANDIDATES);
}

#[test]
fn a_box_behind_the_camera_has_no_bounds_and_no_points() {
    let behind = candidates(
        &cube(Vec3::new(0.0, 0.0, 10.0), 2.0),
        Vec3::ZERO,
        VIEWPORT,
        pinhole,
    );
    assert_eq!(
        behind,
        Candidates {
            bounds: None,
            on_screen: false,
            points: Vec::new(),
        }
    );
}

#[test]
fn only_points_inside_the_viewport_are_candidates() {
    // Centred at x ≈ 780 px: its near face projects past the right edge, its
    // left side and its centre do not.
    let edge = candidates(
        &cube(Vec3::new(38.0, 0.0, -10.0), 4.0),
        Vec3::ZERO,
        VIEWPORT,
        pinhole,
    );
    assert!(edge.on_screen);
    assert!(!edge.points.is_empty());
    assert!(
        edge.points
            .iter()
            .all(|point| point.x < VIEWPORT.x && point.x >= 0.0),
        "{:?}",
        edge.points
    );
    let gone = candidates(
        &cube(Vec3::new(80.0, 0.0, -10.0), 2.0),
        Vec3::ZERO,
        VIEWPORT,
        pinhole,
    );
    assert!(!gone.on_screen);
    assert!(gone.points.is_empty());
    assert!(gone.bounds.is_some(), "off screen, but still projected");
}

#[test]
fn a_rotation_is_the_same_as_itself_even_a_hair_off_unit_length() {
    use super::same_rotation;

    // A quarter turn a hair short of unit length, as a rotation read back
    // from a `GlobalTransform` can be: `1 − q·q` is already ~3.4e-7.
    let rotation = Quat::from_xyzw(0.0, 0.707_106_6, 0.0, 0.707_106_6);
    assert!(1.0 - rotation.dot(rotation) > 1.0e-7, "the trap is real");
    assert!(same_rotation(rotation, rotation));
    assert!(
        same_rotation(
            rotation,
            Quat::from_xyzw(-rotation.x, -rotation.y, -rotation.z, -rotation.w)
        ),
        "q and −q are one rotation"
    );
    assert!(!same_rotation(
        rotation,
        rotation.mul_quat(Quat::from_rotation_y(1.0e-4))
    ));
}
