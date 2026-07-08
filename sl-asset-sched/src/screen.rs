//! On-screen importance: the approximate screen pixel area an object covers.
//!
//! This is the raw quantity the reference viewer drives texture / mesh fetch
//! priority and level-of-detail selection by — how large a thing appears on the
//! screen, in pixels. It is deliberately domain-free (it knows nothing about
//! textures, meshes, or discard levels); a higher-level store turns the pixel
//! area into a [`Priority`](crate::Priority) (see the viewer's priority driver)
//! or an LOD tier.
//!
//! Ported from the reference viewer's `LLPipeline::calcPixelArea`
//! (`indra/newview/pipeline.cpp`), together with the pixels-per-radian factor
//! `LLDrawable::sCurPixelAngle` (`indra/newview/lldrawable.cpp`), which is
//! `window_height / camera_vertical_fov`. `LLFace::calcPixelArea` and
//! `LLVOVolume::getPixelArea` are thin callers of the same math.

use core::f32::consts::PI;

/// Below this camera distance (metres) the reference viewer ramps the distance
/// down so very close objects do not all collapse to the same enormous pixel
/// area — `LLPipeline::calcPixelArea`'s `if (dist < 16.f)` branch.
const NEAR_RAMP_DISTANCE: f32 = 16.0;

/// The per-frame screen geometry that converts a world-space apparent size into
/// a pixel count: the reference viewer's `LLDrawable::sCurPixelAngle`, the
/// number of screen pixels spanned by one radian of the camera's vertical field
/// of view. Recompute it whenever the window is resized or the camera's field
/// of view changes (once per frame in the viewer), then reuse it for every
/// object that frame.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `ScreenMetrics`; the `screen::` path is not the public one"
)]
pub struct ScreenMetrics {
    /// Screen pixels per radian of vertical field of view
    /// (`window_height / vertical_fov`).
    pixels_per_radian: f32,
}

impl ScreenMetrics {
    /// Screen metrics for a viewport `viewport_height` pixels tall rendered with
    /// a `vertical_fov` (radians) camera. A non-positive or non-finite field of
    /// view yields zero pixels-per-radian, so every [`pixel_area`] is then zero
    /// rather than infinite or `NaN`.
    ///
    /// [`pixel_area`]: ScreenMetrics::pixel_area
    #[must_use]
    pub fn new(viewport_height: f32, vertical_fov: f32) -> Self {
        let pixels_per_radian = if vertical_fov > 0.0 && vertical_fov.is_finite() {
            viewport_height / vertical_fov
        } else {
            0.0
        };
        Self { pixels_per_radian }
    }

    /// The pixels-per-radian factor (`LLDrawable::sCurPixelAngle`).
    #[must_use]
    pub const fn pixels_per_radian(self) -> f32 {
        self.pixels_per_radian
    }

    /// The approximate screen area, in pixels, covered by an object of world
    /// bounding radius `bounding_radius` (metres) whose centre is
    /// `camera_distance` metres from the camera.
    ///
    /// Mirrors `LLPipeline::calcPixelArea`: the object's apparent angular radius
    /// is `atan(bounding_radius / distance)`, scaled to pixels by
    /// [`pixels_per_radian`], and the returned area is that of the circle of
    /// that pixel radius (`pi * r^2`). Distances under the near-object threshold
    /// (16 m) are ramped down first so very close objects do not all saturate to
    /// the same area.
    ///
    /// [`pixels_per_radian`]: ScreenMetrics::pixels_per_radian
    #[must_use]
    pub fn pixel_area(self, bounding_radius: f32, camera_distance: f32) -> f32 {
        let distance = ramp_near_distance(camera_distance);
        let apparent_angle = ratio_atan(bounding_radius, distance);
        let pixel_radius = apparent_angle * self.pixels_per_radian;
        pixel_radius * pixel_radius * PI
    }
}

/// Ramp a small camera distance down toward zero, matching the reference
/// viewer's near-object shrink in `LLPipeline::calcPixelArea`: below
/// [`NEAR_RAMP_DISTANCE`] the distance is replaced by
/// `(distance / 16)^2 * 16`, which is continuous at the boundary and pushes
/// nearby objects to progressively larger apparent sizes. At or above the
/// threshold (or for a non-finite / negative input) the distance is unchanged.
fn ramp_near_distance(distance: f32) -> f32 {
    if distance.is_finite() && distance > 0.0 && distance < NEAR_RAMP_DISTANCE {
        let scaled = distance / NEAR_RAMP_DISTANCE;
        scaled * scaled * NEAR_RAMP_DISTANCE
    } else {
        distance
    }
}

/// `atan(numerator / denominator)` guarded against a zero denominator: a zero
/// (or non-finite) distance makes an object subtend the full half-angle
/// `pi / 2`, matching the `f32` limit of `atan(+inf)` without dividing by zero
/// or producing a `NaN`.
fn ratio_atan(numerator: f32, denominator: f32) -> f32 {
    if denominator > 0.0 && denominator.is_finite() {
        (numerator / denominator).atan()
    } else {
        PI / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::{NEAR_RAMP_DISTANCE, PI, ScreenMetrics};

    /// Assert two floats agree within a small relative (and absolute) epsilon —
    /// the workspace denies bit-exact float comparison.
    fn assert_close(actual: f32, expected: f32) {
        let tolerance = 1.0e-4 * expected.abs().max(1.0);
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} != {expected} (tolerance {tolerance})"
        );
    }

    /// A representative viewport: 1080 px tall, ~60 degree vertical field of
    /// view — so a little over 1031 pixels per radian.
    fn metrics() -> ScreenMetrics {
        ScreenMetrics::new(1080.0, core::f32::consts::FRAC_PI_3)
    }

    #[test]
    fn pixels_per_radian_is_height_over_fov() {
        let m = ScreenMetrics::new(1000.0, 2.0);
        assert_close(m.pixels_per_radian(), 500.0);
    }

    #[test]
    fn a_non_positive_fov_yields_zero_and_zero_area() {
        let m = ScreenMetrics::new(1080.0, 0.0);
        assert_close(m.pixels_per_radian(), 0.0);
        assert_close(m.pixel_area(4.0, 20.0), 0.0);
    }

    #[test]
    fn pixel_area_shrinks_with_distance() {
        let m = metrics();
        let near = m.pixel_area(2.0, 20.0);
        let far = m.pixel_area(2.0, 80.0);
        assert!(
            near > far,
            "nearer object must cover more pixels: {near} !> {far}"
        );
    }

    #[test]
    fn pixel_area_grows_with_bounding_radius() {
        let m = metrics();
        let small = m.pixel_area(1.0, 40.0);
        let large = m.pixel_area(4.0, 40.0);
        assert!(
            large > small,
            "larger object must cover more pixels: {large} !> {small}"
        );
    }

    #[test]
    fn a_zero_distance_subtends_the_full_half_angle() {
        let m = metrics();
        // atan(+inf) == pi/2, scaled to pixels and squared into a circle area.
        let pixel_radius = (PI / 2.0) * m.pixels_per_radian();
        assert_close(m.pixel_area(3.0, 0.0), pixel_radius * pixel_radius * PI);
    }

    #[test]
    fn the_near_ramp_is_continuous_at_the_threshold() {
        let m = metrics();
        // At the boundary the ramp maps (16/16)^2*16 back to 16, so values just
        // inside and just outside differ only by the function's local slope, not
        // a jump. Sample a small step either side and require a small delta.
        let inside = m.pixel_area(2.0, NEAR_RAMP_DISTANCE - 0.01);
        let outside = m.pixel_area(2.0, NEAR_RAMP_DISTANCE + 0.01);
        assert!(
            (inside - outside).abs() < 200.0,
            "ramp must not jump at {NEAR_RAMP_DISTANCE} m: {inside} vs {outside}"
        );
    }

    #[test]
    fn the_near_ramp_enlarges_very_close_objects() {
        let m = metrics();
        // At 4 m the ramp replaces the distance with (4/16)^2*16 = 1 m, so the
        // object appears far larger than the un-ramped 4 m would give.
        let ramped = m.pixel_area(2.0, 4.0);
        let un_ramped_radius = (2.0_f32 / 4.0).atan() * m.pixels_per_radian();
        let un_ramped = un_ramped_radius * un_ramped_radius * PI;
        assert!(
            ramped > un_ramped * 4.0,
            "near ramp must enlarge a 4 m object: {ramped} vs un-ramped {un_ramped}"
        );
    }
}
