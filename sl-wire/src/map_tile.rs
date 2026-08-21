//! The world-map tile HTTP surface: the file names a viewer requests under
//! the grid's `map-server-url` (login response / `SimulatorFeatures`
//! `OpenSimExtras`), `map-<zoom>-<x>-<y>-objects.jpg`.
//!
//! The viewer (`llworldmipmap.cpp`) composes the URL as the base plus the
//! file name; OpenSim's `MapGetServerConnector` looks the file up by that
//! name and answers `image/jpeg` (or 404). Zoom levels run 1 (one region per
//! tile) to 8, each doubling the region span; `x`/`y` are the tile's
//! lower-left region coordinates, aligned to the tile's span.

/// The `Content-Type` a tile is served with.
pub const MAP_TILE_CONTENT_TYPE: &str = "image/jpeg";

/// The lowest zoom level (one region per tile).
pub const MAP_TILE_MIN_ZOOM: u8 = 1;

/// The highest zoom level the viewer requests.
pub const MAP_TILE_MAX_ZOOM: u8 = 8;

/// One map tile's identity: zoom level and lower-left region coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MapTileRef {
    /// The zoom level, [`MAP_TILE_MIN_ZOOM`]..=[`MAP_TILE_MAX_ZOOM`].
    pub zoom: u8,
    /// The tile's lower-left region X coordinate.
    pub x: u32,
    /// The tile's lower-left region Y coordinate.
    pub y: u32,
}

impl MapTileRef {
    /// A tile reference, if `zoom` is within the supported range.
    #[must_use]
    pub const fn new(zoom: u8, x: u32, y: u32) -> Option<Self> {
        if zoom < MAP_TILE_MIN_ZOOM || zoom > MAP_TILE_MAX_ZOOM {
            return None;
        }
        Some(Self { zoom, x, y })
    }

    /// The number of regions the tile spans along each axis (`2^(zoom-1)`).
    #[must_use]
    pub const fn regions_per_side(self) -> u32 {
        1_u32 << (self.zoom.saturating_sub(1))
    }

    /// The file name the viewer requests, `map-<zoom>-<x>-<y>-objects.jpg`.
    #[must_use]
    pub fn file_name(self) -> String {
        format!("map-{}-{}-{}-objects.jpg", self.zoom, self.x, self.y)
    }

    /// Parses a tile file name (a leading `/` is tolerated); `None` for any
    /// other path or an out-of-range zoom.
    #[must_use]
    pub fn parse_file_name(name: &str) -> Option<Self> {
        let rest = name.strip_prefix('/').unwrap_or(name);
        let rest = rest.strip_prefix("map-")?.strip_suffix("-objects.jpg")?;
        let mut parts = rest.split('-');
        let zoom = parts.next()?.parse().ok()?;
        let x = parts.next()?.parse().ok()?;
        let y = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Self::new(zoom, x, y)
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::MapTileRef;

    #[test]
    fn file_names_round_trip() {
        let tile = MapTileRef::new(1, 1000, 1001).unwrap_or(MapTileRef {
            zoom: 1,
            x: 0,
            y: 0,
        });
        assert_eq!(tile.file_name(), "map-1-1000-1001-objects.jpg");
        assert_eq!(
            MapTileRef::parse_file_name("map-1-1000-1001-objects.jpg"),
            Some(tile)
        );
        assert_eq!(
            MapTileRef::parse_file_name("/map-1-1000-1001-objects.jpg"),
            Some(tile)
        );
        assert_eq!(
            MapTileRef::parse_file_name("map-8-0-0-objects.jpg").map(MapTileRef::regions_per_side),
            Some(128)
        );
        assert_eq!(tile.regions_per_side(), 1);
    }

    #[test]
    fn junk_is_rejected() {
        for junk in [
            "map-0-1-1-objects.jpg",
            "map-9-1-1-objects.jpg",
            "map-1-1-objects.jpg",
            "map-1-1-1-1-objects.jpg",
            "map-1-a-1-objects.jpg",
            "map-1-1-1-objects.png",
            "tile-1-1-1-objects.jpg",
            "",
        ] {
            assert_eq!(MapTileRef::parse_file_name(junk), None, "{junk}");
        }
        assert!(MapTileRef::new(0, 0, 0).is_none());
    }
}
