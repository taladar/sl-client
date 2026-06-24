//! Geometry value types the wire carries.
//!
//! [`Direction`] (a 3-D facing direction) and [`GlobalCoordinates`] (a
//! grid-global position in metres) now live in the shared `sl-types` crate
//! (`sl_types::map`); they are re-exported here (and onward through `sl-proto`
//! and the runtimes) so the existing `sl_wire::…` / `sl_proto::…` paths are
//! unchanged. This module additionally keeps the region-local narrowing helper
//! the LLSD codecs use.

pub use sl_types::map::{Direction, GlobalCoordinates};

/// Narrows a region-local-metre `f64` to the `f32` a region-local coordinate
/// uses. A region-local offset is a small (0..256) in-range metre value, so the
/// narrowing is exact for the values the grid sends. Shared with the LLSD codecs
/// that read region-local positions from `f64` reals (e.g. `RemoteParcelRequest`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "a region-local offset is a small (0..256) in-range metre value"
)]
pub(crate) const fn narrow(meters: f64) -> f32 {
    meters as f32
}

#[cfg(test)]
mod tests {
    use super::{Direction, GlobalCoordinates};
    use pretty_assertions::assert_eq;
    use sl_types::map::{GridCoordinates, RegionCoordinates};

    #[test]
    fn direction_components_round_trip() {
        let direction = Direction::new(1.0, -2.5, 0.25);
        // Compare bit patterns: `float_cmp` forbids an exact `==` on the floats.
        assert_eq!(direction.x().to_bits(), 1.0_f32.to_bits());
        assert_eq!(direction.y().to_bits(), (-2.5_f32).to_bits());
        assert_eq!(direction.z().to_bits(), 0.25_f32.to_bits());
    }

    #[test]
    fn direction_normalized_is_unit_length() {
        // 3-4-5 triangle: length 5, so the unit vector is exactly (0, 0.6, 0.8).
        let normalized = Direction::new(0.0, 3.0, 4.0).normalized();
        assert_eq!(normalized, Some(Direction::new(0.0, 0.6, 0.8)));
    }

    #[test]
    fn direction_zero_has_no_normal() {
        assert_eq!(Direction::ZERO.normalized(), None);
    }

    #[test]
    fn global_from_grid_and_region() {
        let grid = GridCoordinates::new(1000, 1001);
        let region = RegionCoordinates::new(128.5, 64.25, 30.0);
        let global = GlobalCoordinates::from_grid_and_region(grid, region);
        assert_eq!(global, GlobalCoordinates::new(256_128.5, 256_320.25, 30.0));
        // `From` tuple conversion agrees with the named constructor.
        assert_eq!(GlobalCoordinates::from((grid, region)), global);
    }

    #[test]
    fn global_from_grid_corner() {
        let grid = GridCoordinates::new(1000, 1001);
        let corner = GlobalCoordinates::from_grid_corner(grid);
        assert_eq!(corner, GlobalCoordinates::new(256_000.0, 256_256.0, 0.0));
        // `From<GridCoordinates>` agrees, and equals the all-zero-region form.
        assert_eq!(GlobalCoordinates::from(grid), corner);
        assert_eq!(
            GlobalCoordinates::from_grid_and_region(grid, RegionCoordinates::new(0.0, 0.0, 0.0)),
            corner
        );
    }

    #[test]
    fn global_split_inverts_combine() {
        let grid = GridCoordinates::new(1000, 1001);
        let region = RegionCoordinates::new(128.5, 64.25, 30.0);
        let global = GlobalCoordinates::from_grid_and_region(grid, region);
        assert_eq!(global.split(), Some((grid, region)));
    }

    #[test]
    fn global_split_rejects_negative() {
        assert_eq!(GlobalCoordinates::new(-1.0, 0.0, 0.0).split(), None);
    }
}
