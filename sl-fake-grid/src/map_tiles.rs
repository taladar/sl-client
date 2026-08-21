//! The world-map tile surface: `GET /map-<zoom>-<x>-<y>-objects.jpg` under
//! the login URI, which doubles as the grid's `map-server-url`.
//!
//! Tiles are whatever the builder registered plus a stock zoom-1 tile per
//! configured region (an embedded JPEG), so a viewer's world map shows the
//! grid's regions without any image pipeline in the fake grid. Absent tiles
//! answer 404 like OpenSim's `MapGetServerConnector`.

use std::collections::HashMap;

use bytes::Bytes;
use sl_wire::{MAP_TILE_CONTENT_TYPE, MapTileRef};

use crate::http_answer::HttpAnswer;

/// The stock tile served for every configured region at zoom 1: a 256²
/// baseline JPEG of a green island on blue water.
pub const STOCK_TILE_JPEG: &[u8] = include_bytes!("../fixtures/tile.jpg");

/// The `Cache-Control` max-age tiles are served with, in seconds — long
/// enough that the viewer's disk cache (`sl-map-apis` honours the HTTP cache
/// policy) does not re-fetch every tile every frame.
const TILE_MAX_AGE_SECS: u32 = 3600;

/// The registered tiles.
#[derive(Debug, Clone, Default)]
pub(crate) struct MapTileStore {
    /// Tile bytes by reference.
    tiles: HashMap<MapTileRef, Bytes>,
}

impl MapTileStore {
    /// Registers (or replaces) a tile.
    pub(crate) fn insert(&mut self, tile: MapTileRef, jpeg: Bytes) {
        self.tiles.insert(tile, jpeg);
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
        let Some(jpeg) = self.get(tile) else {
            return Some(HttpAnswer::status(404));
        };
        let etag = format!("\"{}-{}\"", tile.file_name(), jpeg.len());
        let mut answer = HttpAnswer::ok(MAP_TILE_CONTENT_TYPE, jpeg.clone())
            .header(
                "cache-control",
                format!("public, max-age={TILE_MAX_AGE_SECS}"),
            )
            .header("etag", etag)
            .header("content-length", jpeg.len().to_string());
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

    use super::{MapTileStore, STOCK_TILE_JPEG};

    #[test]
    fn stock_tile_is_a_jpeg_and_is_served_with_cache_headers() -> Result<(), String> {
        assert_eq!(STOCK_TILE_JPEG.get(..2), Some(&[0xFF, 0xD8][..]));
        let mut store = MapTileStore::default();
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
            Some(404)
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
