//! The world-map tile surface: `GET /map-<zoom>-<x>-<y>-objects.jpg` under
//! the login URI, which doubles as the grid's `map-server-url`.
//!
//! Tiles are whatever the builder registered plus a stock zoom-1 tile per
//! configured region (an embedded JPEG), so a viewer's world map shows the
//! grid's regions without any image pipeline in the fake grid. A tile it does
//! not have is answered as the imitated grid's tile server answers one
//! ([`AbsentTile`]): Second Life's with a `403`, OpenSim's with a tile of
//! plain water — and since only zoom 1 is seeded, that is every coarser zoom
//! too.

use std::collections::HashMap;

use bytes::Bytes;
use sl_wire::{MAP_TILE_CONTENT_TYPE, MapTileRef};

use crate::http_answer::HttpAnswer;
use crate::imitates::{AbsentTile, MapPolicy};

/// The tile OpenSim serves where it has none: 256² of plain water, as the
/// local OpenSim sent it (2026-10-08).
pub const BLANK_TILE_JPEG: &[u8] = include_bytes!("../fixtures/blank-tile.jpg");

/// The body of the `403` Second Life's content network refuses an absent
/// tile with.
const ACCESS_DENIED_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
    <Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>";

/// The stock tile served for every configured region at zoom 1: a 256²
/// baseline JPEG of a green island on blue water.
pub const STOCK_TILE_JPEG: &[u8] = include_bytes!("../fixtures/tile.jpg");

/// The `Cache-Control` max-age Second Life's tiles are served with, in
/// seconds: a day, which is the most the live server was seen to state.
const TILE_MAX_AGE_SECS: u32 = 86_400;

/// The registered tiles.
#[derive(Debug, Clone, Default)]
pub(crate) struct MapTileStore {
    /// Tile bytes by reference.
    tiles: HashMap<MapTileRef, Bytes>,
    /// What a tile the store does not have is answered with.
    absent: AbsentTile,
    /// Whether a tile comes with cache headers.
    cache_headers: bool,
}

impl MapTileStore {
    /// Registers (or replaces) a tile.
    pub(crate) fn insert(&mut self, tile: MapTileRef, jpeg: Bytes) {
        self.tiles.insert(tile, jpeg);
    }

    /// Answers absent tiles, and sends cache headers, as `policy`'s grid
    /// does.
    pub(crate) const fn imitate(&mut self, policy: &MapPolicy) {
        self.absent = policy.absent_tile;
        self.cache_headers = policy.tile_cache_headers;
    }

    /// Registers the stock tile for a region at zoom 1 unless the builder
    /// already supplied one.
    pub(crate) fn seed_region(&mut self, grid_x: u32, grid_y: u32) {
        if let Some(tile) = MapTileRef::new(1, grid_x, grid_y) {
            self.tiles
                .entry(tile)
                .or_insert_with(|| Bytes::from_static(STOCK_TILE_JPEG));
        }
    }

    /// The bytes of a tile, if registered.
    pub(crate) fn get(&self, tile: MapTileRef) -> Option<&Bytes> {
        self.tiles.get(&tile)
    }

    /// Answers a request whose path names a tile: `GET`/`HEAD` with the
    /// JPEG and cache headers, 404 for an unregistered tile, 405 for any
    /// other method. `None` when the path is not a tile path at all.
    pub(crate) fn answer(&self, method: &str, path: &str) -> Option<HttpAnswer> {
        let tile = MapTileRef::parse_file_name(path)?;
        if method != "GET" && method != "HEAD" {
            return Some(HttpAnswer::status(405));
        }
        let jpeg = match (self.get(tile), self.absent) {
            (Some(jpeg), _) => jpeg.clone(),
            (None, AbsentTile::BlankTile) => Bytes::from_static(BLANK_TILE_JPEG),
            (None, AbsentTile::Forbidden) => {
                let mut refusal = HttpAnswer::ok("application/xml", ACCESS_DENIED_XML);
                refusal.status = 403;
                if method == "HEAD" {
                    refusal.body = Bytes::new();
                }
                return Some(refusal);
            }
        };
        let mut answer = HttpAnswer::ok(MAP_TILE_CONTENT_TYPE, jpeg.clone())
            .header("content-length", jpeg.len().to_string());
        if self.cache_headers {
            answer = answer
                .header("cache-control", format!("max-age={TILE_MAX_AGE_SECS}"))
                .header("etag", format!("\"{}-{}\"", tile.file_name(), jpeg.len()))
                .header("last-modified", "Thu, 08 Oct 2026 00:00:00 GMT");
        }
        if method == "HEAD" {
            answer.body = Bytes::new();
        }
        Some(answer)
    }
}

#[cfg(test)]
mod test {
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_wire::MapTileRef;

    use super::{BLANK_TILE_JPEG, MapTileStore, STOCK_TILE_JPEG};
    use crate::imitates::ImitatedGrid;

    /// A store answering as `grid`'s tile server does.
    fn store_of(grid: ImitatedGrid) -> MapTileStore {
        let mut store = MapTileStore::default();
        store.imitate(&grid.map_policy());
        store
    }

    /// Second Life sends a tile with cache headers and OpenSim with none.
    #[test]
    fn only_second_life_sends_cache_headers() -> Result<(), String> {
        for (grid, expected) in [
            (ImitatedGrid::SecondLife, true),
            (ImitatedGrid::OpenSim, false),
        ] {
            let mut store = store_of(grid);
            store.seed_region(1000, 1000);
            let answer = store
                .answer("GET", "/map-1-1000-1000-objects.jpg")
                .ok_or("tile path not recognised")?;
            for name in ["cache-control", "etag", "last-modified"] {
                assert_eq!(
                    answer.headers.iter().any(|(header, _)| *header == name),
                    expected,
                    "{grid:?} {name}"
                );
            }
        }
        Ok(())
    }

    /// A tile the server does not have is refused by Second Life and
    /// answered with plain water by OpenSim.
    #[test]
    fn an_absent_tile_is_answered_as_each_grid_does() -> Result<(), String> {
        let refused = store_of(ImitatedGrid::SecondLife)
            .answer("GET", "/map-1-100-100-objects.jpg")
            .ok_or("tile path not recognised")?;
        assert_eq!(
            (refused.status, refused.content_type),
            (403, "application/xml")
        );
        let water = store_of(ImitatedGrid::OpenSim)
            .answer("GET", "/map-1-100-100-objects.jpg")
            .ok_or("tile path not recognised")?;
        assert_eq!((water.status, water.content_type), (200, "image/jpeg"));
        assert_eq!(water.body.as_ref(), BLANK_TILE_JPEG);
        assert_eq!(BLANK_TILE_JPEG.get(..2), Some(&[0xFF, 0xD8][..]));
        Ok(())
    }

    #[test]
    fn stock_tile_is_a_jpeg_and_is_served_with_cache_headers() -> Result<(), String> {
        assert_eq!(STOCK_TILE_JPEG.get(..2), Some(&[0xFF, 0xD8][..]));
        let mut store = store_of(ImitatedGrid::SecondLife);
        store.seed_region(1000, 1000);
        let answer = store
            .answer("GET", "/map-1-1000-1000-objects.jpg")
            .ok_or("tile path not recognised")?;
        assert_eq!(answer.status, 200);
        assert_eq!(answer.content_type, "image/jpeg");
        assert_eq!(answer.body.len(), STOCK_TILE_JPEG.len());
        assert!(
            answer
                .headers
                .iter()
                .any(|(name, _)| *name == "cache-control")
        );
        let head = store
            .answer("HEAD", "/map-1-1000-1000-objects.jpg")
            .ok_or("tile path not recognised")?;
        assert_eq!(head.status, 200);
        assert!(head.body.is_empty());
        Ok(())
    }

    #[test]
    fn missing_tiles_and_other_paths() -> Result<(), String> {
        let mut store = MapTileStore::default();
        store.seed_region(1000, 1000);
        assert_eq!(
            store
                .answer("GET", "/map-2-1000-1000-objects.jpg")
                .map(|a| a.status),
            Some(403)
        );
        assert_eq!(
            store
                .answer("POST", "/map-1-1000-1000-objects.jpg")
                .map(|a| a.status),
            Some(405)
        );
        assert!(store.answer("GET", "/other").is_none());
        let custom = MapTileRef::new(2, 1000, 1000).ok_or("bad zoom")?;
        store.insert(custom, Bytes::from_static(b"jpg"));
        assert_eq!(
            store
                .answer("GET", "/map-2-1000-1000-objects.jpg")
                .map(|a| a.body),
            Some(Bytes::from_static(b"jpg"))
        );
        Ok(())
    }
}
