//! The world-map tile service: fetches and caches the grid's map-tile imagery
//! (`map-<zoom>-<x>-<y>-objects.jpg`) for [`crate::world_map`].
//!
//! The fetch and the two-level (memory + disk, `http-cache-semantics`-aware)
//! cache are the sibling `sl-map-tools` workspace's
//! [`sl_map_apis::map_tiles::MapTileCache`] — deliberately reused rather than
//! writing a third tile fetcher. That API is async (tokio + reqwest), while
//! the viewer's I/O convention is plain worker threads, so a dedicated worker
//! thread owns a small current-thread tokio runtime and the cache, and talks
//! to the ECS over std mpsc channels: the ECS side sends `TileKey`s, the
//! worker answers with a `TileAnswer` — a decoded RGBA raster, a definitive
//! "missing", or a failed fetch.
//!
//! The last of those three is kept apart from the second on purpose. A tile
//! the server does not have is settled and never asked for again; a fetch that
//! timed out or answered 500 is not, and folding the two together would blank
//! that region on the map for the rest of the session. A failed slot therefore
//! keeps a failure count and a retry stamp, and `WorldMapTiles::request` sends
//! it again once the backoff has passed, up to `MAX_TILE_FETCH_ATTEMPTS`
//! fetches.
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

/// What the worker found out about one tile.
#[derive(Debug)]
pub(crate) enum TileAnswer {
    /// Decoded imagery.
    Raster(TileRaster),
    /// The server definitively has no tile here — settled, never re-asked.
    Missing,
    /// The fetch failed (timeout, HTTP error, DNS, a bad level, a decode that
    /// did not yield an image). Retryable, and logged where it happened.
    Failed,
}

/// A tile's fetch state on the ECS side.
#[derive(Debug, Clone)]
pub(crate) enum TileState {
    /// Requested from the worker; no answer yet.
    Pending,
    /// The server definitively has no tile here (cached absence).
    Missing,
    /// The last fetch failed. `retry_at` is the earliest [`WorldMapTiles::frame`]
    /// stamp at which [`WorldMapTiles::request`] sends it again, or `None` once
    /// the retry budget is spent — a given-up slot is still evictable, so
    /// panning away and back eventually asks again.
    Failed {
        /// The frame stamp a retry may be sent at; `None` once given up.
        retry_at: Option<u64>,
    },
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
    /// How many fetches of this tile have failed so far. Survives the
    /// `Pending` state a retry passes through, and resets on any answer that
    /// settles the tile.
    failures: u32,
}

/// The channel ends owned by the ECS side while a worker runs.
struct ServiceHandle {
    /// Requests toward the worker (dropping it ends the worker loop).
    request_tx: Sender<TileKey>,
    /// Answers from the worker.
    response_rx: Receiver<(TileKey, TileAnswer)>,
    /// The worker thread (detached on drop; kept for liveness diagnostics).
    _thread: JoinHandle<()>,
}

/// Evict least-recently-used tiles above this many resident slots (each ready
/// tile is a 256×256 RGBA raster, 256 KiB). Only settled slots count and only
/// settled slots are evicted: dropping a `Pending` one loses nothing but makes
/// [`WorldMapTiles::request`] send a fetch that is already in flight.
const MAX_RESIDENT_TILES: usize = 384;

/// How many times one tile is fetched before the map gives up on it: the first
/// attempt plus three retries.
const MAX_TILE_FETCH_ATTEMPTS: u32 = 4;

/// The wait before the first retry, in [`WorldMapTiles::frame`] stamps — one
/// per drain, so one per frame while the world map plugin runs (≈1 s at 60
/// FPS). Each further retry doubles it.
const RETRY_BACKOFF_FRAMES: u64 = 60;

/// The frames to wait before the `failures`-th retry of a tile fetch, doubling
/// from [`RETRY_BACKOFF_FRAMES`].
const fn retry_backoff_frames(failures: u32) -> u64 {
    match RETRY_BACKOFF_FRAMES.checked_shl(failures.saturating_sub(1)) {
        Some(frames) => frames,
        None => u64::MAX,
    }
}

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
        let (response_tx, response_rx) = unbounded::<(TileKey, TileAnswer)>();
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

    /// Requests a tile if it is not already resident or in flight. A slot
    /// whose last fetch *failed* is requested again once its backoff has
    /// passed and while its retry budget lasts; a `Missing` one never is.
    pub(crate) fn request(&mut self, key: TileKey) {
        let Some(handle) = &self.handle else {
            return;
        };
        let failures = match self.tiles.get(&key) {
            None => 0,
            Some(slot) => match slot.state {
                TileState::Failed {
                    retry_at: Some(retry_at),
                } if retry_at <= self.frame => slot.failures,
                _ => return,
            },
        };
        if handle.request_tx.send(key).is_ok() {
            self.tiles.insert(
                key,
                TileSlot {
                    state: TileState::Pending,
                    last_used: self.frame,
                    failures,
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
            for (key, answer) in handle.response_rx.try_iter() {
                arrived = true;
                let failed_before = self.tiles.get(&key).map_or(0, |slot| slot.failures);
                let (state, failures) = match answer {
                    TileAnswer::Raster(raster) => (TileState::Ready(Arc::new(raster)), 0),
                    TileAnswer::Missing => (TileState::Missing, 0),
                    TileAnswer::Failed => {
                        let failures = failed_before.saturating_add(1);
                        let retry_at = if failures >= MAX_TILE_FETCH_ATTEMPTS {
                            warn!(
                                "world map: giving up on the tile at level {} ({}, {}) after {} \
                                 failed fetches",
                                key.level, key.x, key.y, failures
                            );
                            None
                        } else {
                            Some(frame.saturating_add(retry_backoff_frames(failures)))
                        };
                        (TileState::Failed { retry_at }, failures)
                    }
                };
                self.tiles.insert(
                    key,
                    TileSlot {
                        state,
                        last_used: frame,
                        failures,
                    },
                );
            }
        }
        self.evict_settled_tiles();
        arrived
    }

    /// Evicts the least-recently-used **settled** tiles above the residency
    /// cap. In-flight (`Pending`) slots are never candidates: they hold no
    /// raster, and dropping one would let [`Self::request`] send a second
    /// fetch for a tile whose first is still running — which is exactly what
    /// panning a requested tile off-screen and back used to do.
    fn evict_settled_tiles(&mut self) {
        let mut stamps: Vec<(u64, TileKey)> = self
            .tiles
            .iter()
            .filter(|(_key, slot)| !matches!(slot.state, TileState::Pending))
            .map(|(key, slot)| (slot.last_used, *key))
            .collect();
        let excess = stamps.len().saturating_sub(MAX_RESIDENT_TILES);
        if excess == 0 {
            return;
        }
        stamps.sort_unstable_by_key(|(stamp, _key)| *stamp);
        for (_stamp, key) in stamps.into_iter().take(excess) {
            self.tiles.remove(&key);
        }
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
/// [`MapTileCache`]; answers each requested `TileKey` with a [`TileAnswer`].
/// Ends when the request channel closes (the ECS side dropped or restarted the
/// service).
fn tile_worker(
    base_url: &str,
    cache_dir: PathBuf,
    requests: &Receiver<TileKey>,
    responses: &Sender<(TileKey, TileAnswer)>,
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
        let answer = fetch_tile(&runtime, &mut cache, key);
        if responses.send((key, answer)).is_err() {
            return;
        }
    }
}

/// Fetches and decodes one tile. A tile the server does not have answers
/// [`TileAnswer::Missing`] and is never asked for again; a timeout, an HTTP
/// error or an undecodable level answers [`TileAnswer::Failed`], is logged,
/// and stays retryable for this session.
fn fetch_tile(
    runtime: &tokio::runtime::Runtime,
    cache: &mut MapTileCache,
    key: TileKey,
) -> TileAnswer {
    let zoom = match ZoomLevel::try_new(key.level) {
        Ok(zoom) => zoom,
        Err(error) => {
            warn!("world map: bad tile level {}: {error}", key.level);
            return TileAnswer::Failed;
        }
    };
    let descriptor = MapTileDescriptor::new(zoom, GridCoordinates::new(key.x, key.y));
    match runtime.block_on(cache.get_map_tile(&descriptor)) {
        Ok(Some(tile)) => {
            let rgba = tile.image().to_rgba8();
            let (width, height) = (rgba.width(), rgba.height());
            TileAnswer::Raster(TileRaster {
                width,
                height,
                data: rgba.into_raw(),
            })
        }
        Ok(None) => TileAnswer::Missing,
        Err(error) => {
            warn!(
                "world map: tile fetch failed for level {} ({}, {}): {error}",
                key.level, key.x, key.y
            );
            TileAnswer::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use std::collections::HashMap;

    use crossbeam_channel::{Receiver, Sender, unbounded};
    use pretty_assertions::assert_eq;

    use super::{
        MAX_RESIDENT_TILES, MAX_TILE_FETCH_ATTEMPTS, RETRY_BACKOFF_FRAMES, ServiceHandle,
        TileAnswer, TileKey, TileState, WorldMapTiles,
    };
    use crate::world_map_math::TileRaster;

    /// A store wired to a *fake* worker. The returned ends are the worker's:
    /// the receiver sees what the store asked for, the sender answers it.
    fn wired_store() -> (
        WorldMapTiles,
        Receiver<TileKey>,
        Sender<(TileKey, TileAnswer)>,
    ) {
        let (request_tx, request_rx) = unbounded::<TileKey>();
        let (response_tx, response_rx) = unbounded::<(TileKey, TileAnswer)>();
        let thread = std::thread::Builder::new()
            .name("world-map-tiles-test".to_owned())
            .spawn(|| ())
            .expect("spawning the placeholder worker thread");
        let tiles = WorldMapTiles {
            handle: Some(ServiceHandle {
                request_tx,
                response_rx,
                _thread: thread,
            }),
            base_url: Some("http://tiles.invalid/".to_owned()),
            tiles: HashMap::new(),
            frame: 0,
        };
        (tiles, request_rx, response_tx)
    }

    /// A tile key at level 1.
    const fn key(x: u32, y: u32) -> TileKey {
        TileKey { level: 1, x, y }
    }

    /// A 1×1 opaque white raster — enough to be `Ready`.
    fn raster() -> TileRaster {
        TileRaster {
            width: 1,
            height: 1,
            data: vec![255, 255, 255, 255],
        }
    }

    /// Drains `frames` times, advancing the store's LRU / retry stamp.
    fn advance(tiles: &mut WorldMapTiles, frames: u64) {
        let mut frame = 0_u64;
        while frame < frames {
            tiles.drain();
            frame = frame.saturating_add(1);
        }
    }

    /// Answers `key` with `answer` and drains it into the store.
    fn answer(
        tiles: &mut WorldMapTiles,
        responses: &Sender<(TileKey, TileAnswer)>,
        tile: TileKey,
        answer: TileAnswer,
    ) {
        responses
            .send((tile, answer))
            .expect("the store holds the response receiver");
        assert!(tiles.drain(), "an answer is an arrival");
    }

    #[test]
    fn a_failed_fetch_is_requested_again_once_its_backoff_has_passed() {
        let (mut tiles, requests, responses) = wired_store();
        tiles.request(key(1000, 1000));
        assert_eq!(requests.try_iter().count(), 1);
        answer(&mut tiles, &responses, key(1000, 1000), TileAnswer::Failed);
        assert!(matches!(
            tiles.state(key(1000, 1000)),
            Some(TileState::Failed { retry_at: Some(_) })
        ));

        // Within the backoff the tile is not re-sent...
        tiles.request(key(1000, 1000));
        assert_eq!(requests.try_iter().count(), 0);

        // ...and after it, it is — this is the whole bug: one timeout used to
        // blank that region for the rest of the session.
        advance(&mut tiles, RETRY_BACKOFF_FRAMES);
        tiles.request(key(1000, 1000));
        assert_eq!(requests.try_iter().count(), 1);
        assert!(matches!(
            tiles.state(key(1000, 1000)),
            Some(TileState::Pending)
        ));
    }

    #[test]
    fn a_retried_tile_that_arrives_is_ready_and_forgets_its_failures() {
        let (mut tiles, requests, responses) = wired_store();
        tiles.request(key(1, 2));
        answer(&mut tiles, &responses, key(1, 2), TileAnswer::Failed);
        advance(&mut tiles, RETRY_BACKOFF_FRAMES);
        tiles.request(key(1, 2));
        assert_eq!(
            requests.try_iter().count(),
            2,
            "the first send and the retry"
        );
        answer(
            &mut tiles,
            &responses,
            key(1, 2),
            TileAnswer::Raster(raster()),
        );
        assert!(matches!(tiles.state(key(1, 2)), Some(TileState::Ready(_))));
        assert_eq!(
            tiles.tiles.get(&key(1, 2)).map(|slot| slot.failures),
            Some(0)
        );
    }

    #[test]
    fn a_missing_tile_is_settled_and_never_asked_for_again() {
        let (mut tiles, requests, responses) = wired_store();
        tiles.request(key(7, 7));
        assert_eq!(requests.try_iter().count(), 1);
        answer(&mut tiles, &responses, key(7, 7), TileAnswer::Missing);
        advance(&mut tiles, RETRY_BACKOFF_FRAMES.saturating_mul(8));
        tiles.request(key(7, 7));
        assert_eq!(requests.try_iter().count(), 0);
        assert!(matches!(tiles.state(key(7, 7)), Some(TileState::Missing)));
    }

    #[test]
    fn the_retry_budget_is_spent_after_the_attempt_limit() {
        let (mut tiles, requests, responses) = wired_store();
        let mut sends = 0_usize;
        let mut attempt = 0_u32;
        while attempt < MAX_TILE_FETCH_ATTEMPTS.saturating_add(2) {
            tiles.request(key(3, 4));
            sends = sends.saturating_add(requests.try_iter().count());
            if matches!(tiles.state(key(3, 4)), Some(TileState::Pending)) {
                answer(&mut tiles, &responses, key(3, 4), TileAnswer::Failed);
            }
            // Long enough that only the spent budget can stop a retry.
            advance(&mut tiles, RETRY_BACKOFF_FRAMES.saturating_mul(32));
            attempt = attempt.saturating_add(1);
        }
        assert_eq!(
            sends,
            usize::try_from(MAX_TILE_FETCH_ATTEMPTS).unwrap_or(usize::MAX)
        );
        assert!(matches!(
            tiles.state(key(3, 4)),
            Some(TileState::Failed { retry_at: None })
        ));
    }

    #[test]
    fn an_in_flight_tile_survives_eviction() {
        let (mut tiles, _requests, responses) = wired_store();
        // One tile requested and never answered, then left to go stale.
        tiles.request(key(0, 0));
        advance(&mut tiles, 4);

        // Fill the store past the residency cap with settled tiles, each
        // touched more recently than the pending one.
        let mut filled = 0_u32;
        while filled
            < u32::try_from(MAX_RESIDENT_TILES)
                .unwrap_or(u32::MAX)
                .saturating_add(8)
        {
            let tile = key(filled.saturating_add(1), 0);
            tiles.request(tile);
            answer(&mut tiles, &responses, tile, TileAnswer::Missing);
            filled = filled.saturating_add(1);
        }

        assert!(
            matches!(tiles.state(key(0, 0)), Some(TileState::Pending)),
            "an in-flight tile must not be evicted: re-requesting it would run \
             a second fetch alongside the first"
        );
        let settled = tiles
            .tiles
            .values()
            .filter(|slot| !matches!(slot.state, TileState::Pending))
            .count();
        assert_eq!(settled, MAX_RESIDENT_TILES);
    }

    #[test]
    fn eviction_drops_the_least_recently_used_settled_tile() {
        let (mut tiles, _requests, responses) = wired_store();
        let mut filled = 0_u32;
        while filled < u32::try_from(MAX_RESIDENT_TILES).unwrap_or(u32::MAX) {
            let tile = key(filled, 5);
            tiles.request(tile);
            answer(&mut tiles, &responses, tile, TileAnswer::Raster(raster()));
            filled = filled.saturating_add(1);
        }
        // Touch the oldest so the *second* oldest becomes the victim.
        assert!(tiles.state(key(0, 5)).is_some());
        let tile = key(filled, 5);
        tiles.request(tile);
        answer(&mut tiles, &responses, tile, TileAnswer::Raster(raster()));

        assert!(tiles.state(key(0, 5)).is_some(), "touched, so kept");
        assert!(
            tiles.state(key(1, 5)).is_none(),
            "the oldest untouched tile is the victim"
        );
        assert!(tiles.state(tile).is_some(), "just arrived");
    }

    #[test]
    fn a_request_without_a_worker_records_nothing() {
        let mut tiles = WorldMapTiles::default();
        assert!(!tiles.running());
        tiles.request(key(1, 1));
        assert!(tiles.state(key(1, 1)).is_none());
    }
}
