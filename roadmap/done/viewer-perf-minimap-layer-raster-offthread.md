---
id: viewer-perf-minimap-layer-raster-offthread
title: Minimap layer rasterization hitches the frame (up to 66 ms) during rez
topic: viewer
status: done
origin: Tracy profiling of Aditi rezzing (2026-07-30)
---

Context: [context/viewer.md](../context/viewer.md).

Tracy self-time over the first ~10 s of rezzing on Aditi showed
`sl_client_bevy_viewer::minimap::regen_minimap_layers` with **min 3 µs, mean
1.45 ms, max 66.6 ms, std 8.5 ms** (n=203) — the worst single-frame stall in the
whole capture, from the O(prims) object-layer plot loop rasterising on the main
thread during a rez burst.

**Done (2026-07-30).** The three content layers now rasterise **off the frame
thread** on the `AsyncComputeTaskPool`, mirroring the existing
`composite_minimap` pattern; `regen_minimap_layers` no longer plots a single
pixel on the main thread.

1. **Off-thread rasterisation.** When a layer is due, the main thread snapshots
   only that layer's inputs into owned `Send` data and spawns one background
   task that builds the `LayerRaster`:
   - object layer → `ObjectLayerInput` (per-object `rel_east/rel_north/up/flags/
     scale/water_height` for the objects that pass the on-map + vertical-cull
     filter) → `build_object_layer` (the disc-plotting loop — the 66 ms spike);
   - parcel layer → `ParcelLayerInput` (per-region origin offset + cloned
     `ParcelOverlayGrid`) → `build_parcel_layer`;
   - terrain backdrops → `TerrainRegionSample` (cloned `TerrainPatch`es +
     `TerrainComposition` + water height) → `build_terrain_maps`
     (`build_terrain_map` refactored to take an owned snapshot).

   Each layer keeps a `Task` field plus pending capture-centre/tpm; on
   completion the poll publishes the fresh `Arc`, **promotes the capture
   geometry together with the raster** (so the compositor always samples a
   raster against the centre/tpm it was built for), and sets `last_stamp = None`
   to force one recomposite. Coalesced like the composite: one task per layer in
   flight, a re-dirty coalesces into the next spawn.

2. **Stationary throttle (object layer).** The object layer is the only one on a
   bare 0.5 s timer, so it gained an explicit throttle: at a timer fire it skips
   the rebuild when the view pose (camera position + heading, avatar position,
   connected-region count) is unchanged **and** no real object change has landed
   since the last rebuild **and** it is not explicitly dirtied. A real move, a
   new neighbour region, a rez/move (`ObjectState::is_changed`, accumulated
   across frames so a change between timer fires is never lost), or an explicit
   dirty flag all keep it rebuilding — currentness is never traded away, only
   idle churn. The parcel layer (rebuild on overlay-change/centre-moved > 3 m)
   and terrain backdrops (rebuild on terrain-revision change) are already
   movement/data-gated, so they inherit the stationary property without new
   code.

Client-side logic unit-tested (`cargo test -p sl-client-bevy-viewer`):
`ObjectPose::is_stationary_from` (unchanged/jitter → stationary; real
move/turn/new-region/avatar-move/appearing-avatar → rebuild) and
`build_object_layer` (owned dot at centre, empty layer transparent).

**Live acceptance (owner-run):** confirm with a ≤10 s `tracy-capture` capture
during active rez on Aditi — `regen_minimap_layers` max/std should collapse to
near its mean (the disc-plotting loop no longer runs on the frame thread; only
the O(prims) transform-read snapshot remains), and the minimap must still update
live while moving.
