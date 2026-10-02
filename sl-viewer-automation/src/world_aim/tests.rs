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
fn the_camera_holds_while_its_points_stay_put_on_screen() {
    use super::points_hold;

    // A ground point three metres ahead and a little below, and a prim corner
    // ten metres off: what a pursuit would click.
    let points = [Vec3::new(0.5, -1.5, -3.0), Vec3::new(-2.0, 0.5, -10.0)];
    let eye_moved = |by: Vec3| move |point: Vec3| pinhole(point - by);
    assert!(
        points_hold(&points, pinhole, eye_moved(Vec3::new(0.002, 0.001, 0.0))),
        "the head's sway (2 mm a frame, measured on aditi) is still: ~0.07 px here"
    );
    assert!(
        !points_hold(&points, pinhole, eye_moved(Vec3::new(0.05, 0.0, 0.0))),
        "five centimetres (~1.7 px on the near point) is a camera move"
    );
    assert!(
        !points_hold(&points, pinhole, eye_moved(Vec3::new(0.0, 0.0, -3.5))),
        "a point passing behind the camera is a move"
    );
    assert!(
        points_hold(&[], pinhole, eye_moved(Vec3::splat(10.0))),
        "with nothing to act on, nothing moves on screen"
    );
}

#[test]
fn an_eye_is_inside_a_box_only_within_its_faces() {
    use super::eye_inside;

    let turned = Affine3A::from_scale_rotation_translation(
        Vec3::new(0.5, 2.0, 0.5),
        Quat::from_rotation_y(core::f32::consts::FRAC_PI_4),
        Vec3::new(10.0, 1.0, -3.0),
    );
    assert!(
        eye_inside(&turned, Vec3::new(10.0, 1.9, -3.0)),
        "near the top"
    );
    assert!(
        !eye_inside(&turned, Vec3::new(10.0, 2.1, -3.0)),
        "just above it"
    );
    // 0.3 m along x is outside the 0.5 m box's unturned half-width of 0.25,
    // but turned 45° its faces' diagonal reaches 0.35.
    assert!(eye_inside(&turned, Vec3::new(10.3, 1.0, -3.0)));
    assert!(!eye_inside(&turned, Vec3::new(10.4, 1.0, -3.0)));
}

#[test]
fn a_pose_is_still_while_the_box_holds_still_on_screen() {
    use super::Pose;

    // A metre box seven metres ahead: about fourteen pixels across under the
    // pinhole, so it tolerates 5 % of that — 0.7 px.
    let boxed = cube(Vec3::new(0.0, 0.0, -7.0), 1.0);
    let here = Pose::of(&boxed, pinhole);
    let eye_moved = |by: f32| {
        Pose::of(&boxed, move |point| {
            pinhole(point - Vec3::new(by, 0.0, 0.0))
        })
    };
    assert!(
        here.same(&eye_moved(0.001)),
        "a millimetre of eye drift is still"
    );
    assert!(
        here.same(&eye_moved(0.03)),
        "a head's idle sway (~0.5 px here) is still: the probed points stay on the box"
    );
    assert!(
        !here.same(&eye_moved(0.1)),
        "ten centimetres (~1.7 px, an eighth of the box) is not"
    );
    // The same half-pixel move is too much for a box that small on screen.
    let speck = cube(Vec3::new(0.0, 0.0, -70.0), 1.0);
    let speck_here = Pose::of(&speck, pinhole);
    let speck_moved = Pose::of(&speck, |point| pinhole(point - Vec3::new(0.5, 0.0, 0.0)));
    assert!(
        !speck_here.same(&speck_moved),
        "a target a pixel or two across must hold to the pixel tolerance"
    );
}
