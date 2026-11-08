---
id: viewer-terrain-land-cache-on-disk
title: Persist the land-height cache to disk (ground floor available at login)
topic: viewer
status: done
origin: fall-through fix — the in-memory land cache is empty at login (2026-08-12)
refs:
  - viewer-avatar-falls-through-ground
---

Context: [context/viewer.md](../context/viewer.md).

`TerrainState` now keeps an **in-memory** `land_cache` (the last-known land
patch per key), so `land_height` keeps answering across a mid-session region
teardown/rebuild — which stops the avatar ground floor (`physics.rs`) from
dropping to `None` and letting the avatar fall through the terrain while its
patches are momentarily absent.

But that cache is **empty at login**, and that is exactly the window the
observer hit ("initially still fell through the floor, possibly from the
position saved at last logout; after a teleport it was better" — the teleport
gave the terrain time to stream in and populate the floor). Persisting the cache
to disk closes it: on the next login the land floor is available immediately,
before terrain streams.

## Plan

- **Serialize** the land patches (`TerrainPatch` is already `serde`) to a
  **grid-scoped** cache file — region handles are grid-global coordinates and
  collide across grids (aditi vs OpenSim both start near the grid origin), so
  the path must include the grid (e.g.
  `asset_cache_dir("terraincache")/<grid>/…`, keyed off the login grid; see
  `paths.rs`).
- **Load** at startup / on first region entry into `land_cache`, before the
  avatar needs a floor.
- **Save** incrementally (on patch ingestion, debounced) or on a timer / at
  exit; do the I/O **off the main thread** (a task, like the other asset caches)
  so a large region write never hitches a frame.
- **Bound** the size (LRU by region, a cap on cached regions) so it does not
  grow without limit across many regions visited.
- **Staleness**: land is effectively static (only terraforming changes it,
  rarely and slowly); a slightly stale cached height is a fine *floor* (far
  better than a fall-through), and live terrain overrides the cache the instant
  it streams. A terraform (`LayerData` for an already-cached patch) refreshes
  the entry. No explicit invalidation beyond "newest patch wins".

## Implemented (2026-11-08)

`sl-viewer-world-scene/src/terrain_disk_cache.rs` (`TerrainDiskCache`,
scheduled by `WorldScenePlugin` after the terrain fold):

- **Where**: `ViewerPaths::terrain_cache_dir(grid)` =
  `<cache root>/terraincache/<grid>/` (`grid` from
  `sl_account_dirs::grid_dir_name`), one `<handle:016x>.terrain` file per
  region; the assembly inserts the resource with it. `terraincache` is in the
  clear-cache purge list. A viewer with no paths keeps nothing on disk.
- **Format**: a small binary format of its own (magic `SLTL`, version, region
  handle, patch count, then per patch layer code / grid position / edge and the
  `f32` heights) — ~260 KiB per standard region instead of the ~1 MB JSON would
  take. A damaged, foreign-region or other-version file is refused with a
  warning and overwritten by the next save.
- **Load**: the agent's region as soon as `SlIdentity.region_handle` names it
  (seeded from the login response), plus each region's `RegionHandshake` and a
  `RegionChanged` destination; once per region per session, read on the
  `IoTaskPool`; merged into `land_cache` **under** any live patch (a live patch
  that streamed during the read wins). Keying the load off the handshake alone
  did **not** work on aditi: the floor still had `land=None` for the first
  frames of a warm login — most likely the root circuit's handshake arrives
  before the session knows the circuit's handle, stamped zero (the fake grid
  sends the avatar's object first, so it never showed there).
- **Save**: a region whose land streamed / changed is written once quiet for
  5 s (or after 30 s of continuous change), on the `IoTaskPool`, temp file +
  rename; every still-unsaved region synchronously at `AppExit` (in `Last`).
- **Bound**: 128 region files (~33 MiB), pruned least-recently-written after
  each save; a revisited region re-streams and re-saves, so its file stays
  newest. Leftover temporaries older than 10 min are removed by the prune.

Unit tests: round trip (incl. a 32-cell `LandExtended` patch), malformed /
non-land patches left out, damaged / foreign / truncated / trailing / other
version files refused, live patch never replaced by a loaded one, save → fresh
session load answers `land_height`, prune keeps the newest N, save debounce;
`paths::the_terrain_cache_is_per_grid`.

## Verified (2026-11-08)

- **Offline e2e** (`e2e_terrain`
  `the_ground_under_the_avatar_survives_a_relog_before_the_terrain_streams`):
  a viewer learns a flat 37.25 m region, relogs while the fake grid withholds
  the region's LAND layer (`FakeGrid::set_withholds_ground`, new), and the
  agent probe's new `ground_height` (= `TerrainState::land_height` under the
  agent) still reads 37.25 m; a ground pick finding nothing proves no land
  streamed. With the disk load disabled the same stage fails with
  `ground … at None` (checked).
- **aditi** (`SL_VIEWER_LOG_AVATAR_GROUND`): cold login — the first 4 frames
  `land=None`; warm login after the fix — `land=Some(32.45)` from the first
  frame, 0 `land=None`, cache restored 256 patches ~1.2 s before the handshake
  completed. Files land under `terraincache/login.aditi.lindenlab.com/`.
- **OpenSim**: the 4 local regions are saved at exit and re-read with no
  warnings (265 492 bytes each, the exact expected size). Local OpenSim streams
  its land before the avatar's first update, so it never shows the cold window
  (`land=Some` from the first frame even cold).

## Original verify plan

Log in over a fresh session (cold cache), walk immediately — no fall-through
during the terrain-load window. Repeat after a prior session populated the
cache; confirm the floor is present from the first frame
(`SL_VIEWER_LOG_AVATAR_GROUND` shows `land=Some(...)` immediately, not `None`).
Both aditi and OpenSim.
