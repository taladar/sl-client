//! The 64-bit region handle a simulator uses to identify a region by its
//! position on the grid.
//!
//! A region handle packs the global south-west corner of a region (in metres)
//! into a single `u64` as `(global_x << 32) | global_y`. Because the value
//! carries grid-position semantics the compiler can't otherwise see, it lives
//! here as a newtype (mirroring the `sl-types` key wrappers) rather than as a
//! bare `u64`, so a region handle can't be transposed with any other 64-bit
//! field and the grid-coordinate decode is a method on the value itself.

/// A Second Life / OpenSim region handle: the region's global south-west corner
/// in metres packed as `(global_x << 32) | global_y`.
///
/// A handle of `0` is the conventional "not yet known" sentinel the simulator
/// uses before a region's position has been established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct RegionHandle(pub u64);

impl RegionHandle {
    /// Builds a region handle from its raw `u64` wire value.
    #[must_use]
    pub const fn new(handle: u64) -> Self {
        Self(handle)
    }

    /// Returns the raw `u64` wire value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Splits the handle into its global south-west corner in metres,
    /// `(global_x, global_y)`.
    #[must_use]
    pub fn global_coordinates(self) -> (u32, u32) {
        let high = self.0.checked_shr(32).unwrap_or(0);
        let low = self.0 & 0xFFFF_FFFF;
        (
            u32::try_from(high).unwrap_or(u32::MAX),
            u32::try_from(low).unwrap_or(u32::MAX),
        )
    }

    /// Splits the handle into its grid coordinates (region indices), i.e. the
    /// global south-west corner in metres divided by the 256 m region size. For
    /// the typed form, use `sl_types::map::GridCoordinates::from`.
    #[must_use]
    pub fn grid_coordinates(self) -> (u32, u32) {
        let (global_x, global_y) = self.global_coordinates();
        (
            global_x.checked_div(256).unwrap_or(0),
            global_y.checked_div(256).unwrap_or(0),
        )
    }

    /// Builds a region handle from its global south-west corner in metres,
    /// `(global_x, global_y)` — the inverse of [`RegionHandle::global_coordinates`].
    /// Unlike [`RegionHandle::from_grid`], the inputs are already in metres (not
    /// region indices), e.g. the `region_x` / `region_y` fields of the login
    /// response.
    #[must_use]
    pub fn from_global(global_x: u32, global_y: u32) -> Self {
        Self(u64::from(global_x).checked_shl(32).unwrap_or(0) | u64::from(global_y))
    }

    /// Builds a region handle from its grid coordinates (region indices) — the
    /// inverse of [`RegionHandle::grid_coordinates`]. The
    /// `From<sl_types::map::GridCoordinates>` impl is the equivalent typed form.
    #[must_use]
    pub fn from_grid(grid_x: u32, grid_y: u32) -> Self {
        let global_x = u64::from(grid_x).checked_mul(256).unwrap_or(0);
        let global_y = u64::from(grid_y).checked_mul(256).unwrap_or(0);
        Self(global_x.checked_shl(32).unwrap_or(0) | global_y)
    }
}

impl core::fmt::Display for RegionHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::fmt::LowerHex for RegionHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerHex::fmt(&self.0, f)
    }
}

impl core::fmt::UpperHex for RegionHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::UpperHex::fmt(&self.0, f)
    }
}

impl From<sl_types::map::GridCoordinates> for RegionHandle {
    /// Builds the region handle for the south-west corner identified by these
    /// grid coordinates. Always succeeds — every grid index packs into a handle.
    fn from(coordinates: sl_types::map::GridCoordinates) -> Self {
        Self::from_grid(coordinates.x(), coordinates.y())
    }
}

impl From<RegionHandle> for sl_types::map::GridCoordinates {
    /// Decodes the handle's grid coordinates. Infallible now that
    /// [`sl_types::map::GridCoordinates`] holds `u32` indices (the handle's
    /// `global / 256` grid index always fits).
    fn from(handle: RegionHandle) -> Self {
        let (x, y) = handle.grid_coordinates();
        Self::new(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::RegionHandle;
    use pretty_assertions::assert_eq;

    #[test]
    fn grid_round_trips() {
        // Da Boom, the first SL region, sits at grid (1000, 1000).
        let handle = RegionHandle::from_grid(1000, 1000);
        assert_eq!(handle.grid_coordinates(), (1000, 1000));
        // 1000 * 256 = 256000 metres on each axis.
        assert_eq!(handle.global_coordinates(), (256_000, 256_000));
    }

    #[test]
    fn global_round_trips() {
        let handle = RegionHandle::from_global(256_000, 256_256);
        assert_eq!(handle.global_coordinates(), (256_000, 256_256));
        assert_eq!(handle.grid_coordinates(), (1000, 1001));
    }

    #[test]
    fn raw_value_is_packed_global_corner() {
        // (global_x << 32) | global_y, computed without a bare shift.
        let handle = RegionHandle::from_global(256_000, 256_256);
        let expected = u64::from(256_000_u32).checked_shl(32).unwrap_or(0) | u64::from(256_256_u32);
        assert_eq!(handle.get(), expected);
        assert_eq!(RegionHandle::new(handle.get()), handle);
    }

    #[test]
    fn zero_is_the_unknown_sentinel() {
        assert_eq!(RegionHandle::default(), RegionHandle(0));
        assert_eq!(RegionHandle(0).grid_coordinates(), (0, 0));
    }

    #[test]
    fn grid_coordinates_type_round_trips() {
        let coordinates = sl_types::map::GridCoordinates::new(1000, 1001);
        let handle = RegionHandle::from(coordinates);
        assert_eq!(handle.grid_coordinates(), (1000, 1001));
        let back = sl_types::map::GridCoordinates::from(handle);
        assert_eq!(back, coordinates);
    }

    #[test]
    fn grid_coordinates_handles_large_index() {
        // global_x = u32::MAX metres → grid index 16_777_215, far above u16::MAX;
        // now representable since GridCoordinates holds u32.
        let handle = RegionHandle::from_global(u32::MAX, 0);
        let coordinates = sl_types::map::GridCoordinates::from(handle);
        assert_eq!(coordinates.x(), u32::MAX / 256);
        assert_eq!(coordinates.y(), 0);
    }
}
