//! The world-map tile service: fetches and caches the grid's map-tile imagery
//! (`map-<zoom>-<x>-<y>-objects.jpg`) for [`crate::world_map`].
//!
//! The fetch and the two-level (memory + disk, `http-cache-semantics`-aware)
//! cache are the sibling `sl-map-tools` workspace's
//! [`sl_map_apis::map_tiles::MapTileCache`] — deliberately reused rather than
//! writing a third tile fetcher. That API is async (tokio + reqwest), while
//! the viewer's I/O convention is plain worker threads, so a dedicated worker
//! thread owns a small current-thread tokio runtime and the cache, and talks
//! to the ECS over std mpsc channels: the ECS side sends [`TileKey`]s, the
//! worker answers with decoded RGBA rasters (or a definitive "missing").
//!
//! The tile **base URL** is grid-specific: the login response's
//! `map-server-url` (OpenSim announces it whenever its `MapTileURL` is
//! configured — the standalone default), a region's `SimulatorFeatures`
//! `map-server-url` where present (fresher, wins), or the Second Life CDN as
//! the fallback for the main grid. A base-URL change (e.g. features arriving
//! after login) restarts the worker and drops the in-memory tiles; the disk
//! cache is keyed per grid under the viewer cache root so grids never mix.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender, unbounded};

use bevy::prelude::*;
use sl_map_apis::map_tiles::{MapLike as _, MapTileCache};
use sl_types::map::{GridCoordinates, MapTileDescriptor, ZoomLevel};
use tracing::{info, warn};

use crate::world_map_math::TileRaster;

/// One map tile's identity: the mipmap level and the tile's lower-left grid
/// corner (already snapped to the level's span).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TileKey {
    /// The mipmap level (1 = one region per tile, 8 = 128 per edge).
    pub(crate) level: u8,
    /// The tile's lower-left corner grid x.
    pub(crate) x: u32,
    /// The tile's lower-left corner grid y.
    pub(crate) y: u32,
}

/// A tile's fetch state on the ECS side.
#[derive(Debug, Clone)]
pub(crate) enum TileState {
    /// Requested from the worker; no answer yet.
    Pending,
    /// The server definitively has no tile here (cached absence).
    Missing,
    /// Decoded imagery, shared cheaply with the compositor.
    Ready(Arc<TileRaster>),
}

/// One tile slot: its state and when it was last touched (for eviction).
#[derive(Debug)]
struct TileSlot {
    /// The fetch state.
    state: TileState,
    /// The [`WorldMapTiles::frame`] stamp of the last lookup.
    last_used: u64,
}

/// The channel ends owned by the ECS side while a worker runs.
struct ServiceHandle {
    /// Requests toward the worker (dropping it ends the worker loop).
    request_tx: Sender<TileKey>,
    /// Answers from the worker.
    response_rx: Receiver<(TileKey, Option<TileRaster>)>,
    /// The worker thread (detached on drop; kept for liveness diagnostics).
    _thread: JoinHandle<()>,
}

/// Evict least-recently-used tiles above this many resident slots (each ready
/// tile is a 256×256 RGBA raster, 256 KiB).
const MAX_RESIDENT_TILES: usize = 384;

/// The world-map tile store and its background fetch service.
#[derive(Default, Resource)]
pub(crate) struct WorldMapTiles {
    /// The running worker, if any.
    handle: Option<ServiceHandle>,
    /// The base URL the running worker fetches from.
    base_url: Option<String>,
    /// The per-tile states.
    tiles: HashMap<TileKey, TileSlot>,
    /// A monotonic lookup stamp (advanced per drain) driving LRU eviction.
    frame: u64,
}

impl WorldMapTiles {
    /// Ensures a worker for `base_url` runs, restarting (and dropping the
    /// resident tiles) when the URL changed. `cache_dir` is the per-grid disk
    /// cache directory.
    pub(crate) fn ensure_service(&mut self, base_url: &str, cache_dir: PathBuf) {
        if self.base_url.as_deref() == Some(base_url) && self.handle.is_some() {
            return;
        }
        info!("world map: tile service for {base_url}");
        self.handle = None;
        self.tiles.clear();
        self.base_url = Some(base_url.to_owned());
        let (request_tx, request_rx) = unbounded::<TileKey>();
        let (response_tx, response_rx) = unbounded::<(TileKey, Option<TileRaster>)>();
        let url = base_url.to_owned();
        let thread = std::thread::Builder::new()
            .name("world-map-tiles".to_owned())
            .spawn(move || tile_worker(&url, cache_dir, &request_rx, &response_tx));
        match thread {
            Ok(thread) => {
                self.handle = Some(ServiceHandle {
                    request_tx,
                    response_rx,
                    _thread: thread,
                });
            }
            Err(error) => warn!("world map: could not spawn the tile worker: {error}"),
        }
    }

    /// Whether a worker is running (a base URL was resolved).
    pub(crate) const fn running(&self) -> bool {
        self.handle.is_some()
    }

    /// Requests a tile if it is not already resident or in flight.
    pub(crate) fn request(&mut self, key: TileKey) {
        let Some(handle) = &self.handle else {
            return;
        };
        if self.tiles.contains_key(&key) {
            return;
        }
        if handle.request_tx.send(key).is_ok() {
            self.tiles.insert(
                key,
                TileSlot {
                    state: TileState::Pending,
                    last_used: self.frame,
                },
            );
        }
    }

    /// Drains worker answers into the store; returns whether anything arrived
    /// (the compositor's recomposite trigger). Also advances the LRU stamp and
    /// evicts the least-recently-used tiles above the residency cap.
    pub(crate) fn drain(&mut self) -> bool {
        self.frame = self.frame.saturating_add(1);
        let mut arrived = false;
        if let Some(handle) = &self.handle {
            let frame = self.frame;
            for (key, raster) in handle.response_rx.try_iter() {
                arrived = true;
                let state = raster.map_or(TileState::Missing, |raster| {
                    TileState::Ready(Arc::new(raster))
                });
                self.tiles.insert(
                    key,
                    TileSlot {
                        state,
                        last_used: frame,
                    },
                );
            }
        }
        if self.tiles.len() > MAX_RESIDENT_TILES {
            let mut stamps: Vec<(u64, TileKey)> = self
                .tiles
                .iter()
                .map(|(key, slot)| (slot.last_used, *key))
                .collect();
            stamps.sort_unstable_by_key(|(stamp, _key)| *stamp);
            let excess = self.tiles.len().saturating_sub(MAX_RESIDENT_TILES);
            for (_stamp, key) in stamps.into_iter().take(excess) {
                self.tiles.remove(&key);
            }
        }
        arrived
    }

    /// The tile's state, touching its LRU stamp; `None` when never requested.
    pub(crate) fn state(&mut self, key: TileKey) -> Option<TileState> {
        let frame = self.frame;
        self.tiles.get_mut(&key).map(|slot| {
            slot.last_used = frame;
            slot.state.clone()
        })
    }
}

/// The worker loop: owns a current-thread tokio runtime and the shared
/// [`MapTileCache`]; answers each requested [`TileKey`] with a decoded raster
/// (`None` for a definitively missing tile). Ends when the request channel
/// closes (the ECS side dropped or restarted the service).
fn tile_worker(
    base_url: &str,
    cache_dir: PathBuf,
    requests: &Receiver<TileKey>,
    responses: &Sender<(TileKey, Option<TileRaster>)>,
) {
    if let Err(error) = fs_err::create_dir_all(&cache_dir) {
        warn!("world map: could not create the tile cache directory: {error}");
        return;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            warn!("world map: could not build the tile runtime: {error}");
            return;
        }
    };
    let mut cache = MapTileCache::new_with_base_url(cache_dir, None, base_url.to_owned());
    while let Ok(key) = requests.recv() {
        let raster = fetch_tile(&runtime, &mut cache, key);
        if responses.send((key, raster)).is_err() {
            return;
        }
    }
}

/// Fetches and decodes one tile; `None` for a missing tile or any fetch error
/// (an error is logged and treated as missing — the map shows the fallback
/// fill and a later session retries through the cache's freshness rules).
fn fetch_tile(
    runtime: &tokio::runtime::Runtime,
    cache: &mut MapTileCache,
    key: TileKey,
) -> Option<TileRaster> {
    let zoom = match ZoomLevel::try_new(key.level) {
        Ok(zoom) => zoom,
        Err(error) => {
            warn!("world map: bad tile level {}: {error}", key.level);
            return None;
        }
    };
    let descriptor = MapTileDescriptor::new(zoom, GridCoordinates::new(key.x, key.y));
    match runtime.block_on(cache.get_map_tile(&descriptor)) {
        Ok(Some(tile)) => {
            let rgba = tile.image().to_rgba8();
            let (width, height) = (rgba.width(), rgba.height());
            Some(TileRaster {
                width,
                height,
                data: rgba.into_raw(),
            })
        }
        Ok(None) => None,
        Err(error) => {
            warn!(
                "world map: tile fetch failed for level {} ({}, {}): {error}",
                key.level, key.x, key.y
            );
            None
        }
    }
}
