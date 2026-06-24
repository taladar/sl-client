//! Client-local geometry value types that `sl-types` does not (yet) provide.
//!
//! These model two concepts the wire carries that the shared `sl-types`
//! coordinate types do not cover:
//!
//! * [`Direction`] — a 3-D facing direction (the agent/camera *at*-axis), used
//!   by the `look_at` fields. It is a direction, not a position, so it cannot
//!   reuse [`RegionCoordinates`]; the viewer treats it as a full 3-D vector (it
//!   keeps its vertical component — see `LLAgent::resetAxes`), conventionally a
//!   unit vector but the wire does not enforce that.
//! * [`GlobalCoordinates`] — a grid-global position in metres (the viewer's
//!   `LLVector3d` "global" frame: `region_index * 256 + region_local_metres`).
//!   `sl-types` has region-local ([`RegionCoordinates`]) and region-index
//!   ([`GridCoordinates`]) coordinates but no global-metre coordinate, so this
//!   fills the gap.
//!
//! Per the workspace's standing rule these live client-side first (here in
//! `sl-wire`, alongside [`RegionHandle`](crate::RegionHandle) and the other
//! client-local wire newtypes, so both the codec and the session layer can use
//! them); they are candidates to migrate into `sl-types` later (at which point
//! the global-metre coordinate would also subsume the PPS config's global-metre
//! usage in the `sl-map-tools` tree — see the roadmap note).

use sl_types::map::{GridCoordinates, RegionCoordinates};

/// The number of metres along one axis of a region; a grid-index step.
const REGION_SIZE_METERS: f64 = 256.0;

/// A 3-D facing direction in a region's local frame — the direction an avatar
/// faces, as carried by the various `look_at` fields (the viewer's agent/camera
/// *at*-axis). It is a direction, **not** a position: the wire stores three
/// `f32`s and the viewer uses the full 3-D vector (including any vertical
/// component) as the forward axis. It is conventionally a unit vector, but the
/// wire does not enforce normalisation, so the raw components are preserved
/// verbatim for byte-identical round-trips.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Direction {
    /// The x component of the facing direction.
    x: f32,
    /// The y component of the facing direction.
    y: f32,
    /// The z component of the facing direction.
    z: f32,
}

impl Direction {
    /// A zero direction (the wire `(0, 0, 0)` sentinel the viewer replaces with
    /// the current camera axis).
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// Creates a direction from its raw components, without normalising.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// The x component of the facing direction.
    #[must_use]
    pub const fn x(&self) -> f32 {
        self.x
    }

    /// The y component of the facing direction.
    #[must_use]
    pub const fn y(&self) -> f32 {
        self.y
    }

    /// The z component of the facing direction.
    #[must_use]
    pub const fn z(&self) -> f32 {
        self.z
    }

    /// The Euclidean length (magnitude) of the direction vector.
    #[must_use]
    pub fn length(&self) -> f32 {
        self.z
            .mul_add(self.z, self.x.mul_add(self.x, self.y * self.y))
            .sqrt()
    }

    /// The unit-length direction, or `None` when the vector has (near-)zero
    /// length and a direction is therefore undefined.
    #[must_use]
    pub fn normalized(&self) -> Option<Self> {
        let length = self.length();
        if length > f32::EPSILON {
            Some(Self::new(self.x / length, self.y / length, self.z / length))
        } else {
            None
        }
    }
}

/// A grid-global position in metres — the viewer's `LLVector3d` "global" frame,
/// where the value along an axis is `region_grid_index * 256 + region_local`.
/// Held as `f64` to match the wire's double-precision global vectors (the
/// directory/event/pick replies carry `LLVector3d`); the few replies that send
/// a single-precision global position widen to `f64` at the codec boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalCoordinates {
    /// The global x coordinate, in metres (west→east).
    x: f64,
    /// The global y coordinate, in metres (south→north).
    y: f64,
    /// The global z coordinate, in metres (altitude).
    z: f64,
}

impl GlobalCoordinates {
    /// Creates global coordinates from their raw metre components.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// The global x coordinate, in metres.
    #[must_use]
    pub const fn x(&self) -> f64 {
        self.x
    }

    /// The global y coordinate, in metres.
    #[must_use]
    pub const fn y(&self) -> f64 {
        self.y
    }

    /// The global z coordinate, in metres.
    #[must_use]
    pub const fn z(&self) -> f64 {
        self.z
    }

    /// Combines a region's grid index and a region-local position into a global
    /// position (`grid_index * 256 + region_local`). The inverse of
    /// [`split`](Self::split).
    #[must_use]
    pub fn from_grid_and_region(grid: GridCoordinates, region: RegionCoordinates) -> Self {
        Self {
            x: f64::from(grid.x()).mul_add(REGION_SIZE_METERS, f64::from(region.x())),
            y: f64::from(grid.y()).mul_add(REGION_SIZE_METERS, f64::from(region.y())),
            z: f64::from(region.z()),
        }
    }

    /// The grid-global position of a region's south-west **corner** — its
    /// `grid_index * 256` origin at zero altitude. This is the corner the PPS
    /// HUD config uses (`<256 * grid_x, 256 * grid_y, 0>`); it avoids
    /// constructing a throwaway all-zero [`RegionCoordinates`] just to reach
    /// [`from_grid_and_region`](Self::from_grid_and_region).
    #[must_use]
    pub fn from_grid_corner(grid: GridCoordinates) -> Self {
        Self {
            x: f64::from(grid.x()) * REGION_SIZE_METERS,
            y: f64::from(grid.y()) * REGION_SIZE_METERS,
            z: 0.0,
        }
    }

    /// Splits a global position into the containing region's grid index and the
    /// region-local position within it. The inverse of
    /// [`from_grid_and_region`](Self::from_grid_and_region).
    ///
    /// Returns `None` when the global position falls outside the representable
    /// grid (a negative or out-of-`u16`-range region index), which never
    /// happens for a position the grid actually sent.
    #[must_use]
    pub fn split(&self) -> Option<(GridCoordinates, RegionCoordinates)> {
        let grid_x = region_index(self.x)?;
        let grid_y = region_index(self.y)?;
        let local_x = f64::from(grid_x).mul_add(-REGION_SIZE_METERS, self.x);
        let local_y = f64::from(grid_y).mul_add(-REGION_SIZE_METERS, self.y);
        Some((
            GridCoordinates::new(grid_x, grid_y),
            RegionCoordinates::new(narrow(local_x), narrow(local_y), narrow(self.z)),
        ))
    }
}

impl From<(GridCoordinates, RegionCoordinates)> for GlobalCoordinates {
    fn from((grid, region): (GridCoordinates, RegionCoordinates)) -> Self {
        Self::from_grid_and_region(grid, region)
    }
}

impl From<GridCoordinates> for GlobalCoordinates {
    /// Builds the south-west corner of the region (see
    /// [`from_grid_corner`](Self::from_grid_corner)).
    fn from(grid: GridCoordinates) -> Self {
        Self::from_grid_corner(grid)
    }
}

/// The region grid index containing a global-metre coordinate, or `None` when
/// it falls outside the `0..=u16::MAX` grid range (including a non-finite or
/// negative input).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the floored index is checked finite and within u16 range before the cast"
)]
fn region_index(meters: f64) -> Option<u16> {
    let index = (meters / REGION_SIZE_METERS).floor();
    if index.is_finite() && index >= 0.0 && index <= f64::from(u16::MAX) {
        Some(index as u16)
    } else {
        None
    }
}

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
