---
id: viewer-perf-minimap-layer-raster-offthread
title: Minimap layer rasterization hitches the frame (up to 66 ms) during rez
topic: viewer
status: bugs
origin: Tracy profiling of Aditi rezzing (2026-07-30)
---

Context: [context/viewer.md](../context/viewer.md).

Tracy self-time over the first ~10 s of rezzing on Aditi shows
`sl_client_bevy_viewer::minimap::regen_minimap_layers` with **min 3 µs, mean
1.45 ms, max 66.6 ms, std 8.5 ms** (n=203). It is near-free most frames but
occasionally blocks the main thread for **66 ms in a single frame** — the worst
single-frame stall in the whole capture, a visible hitch while the world rezzes.

Root cause: `composite_minimap` already snapshots ECS state and runs the heavy
pixel compositing on the `AsyncComputeTaskPool` (a proper background worker).
But the three **content layers** it composites — the object layer (a loop over
*all* minimap prims plotting a filled dot each), the parcel-line layer, and the
per-region terrain backdrops — are still rasterized **synchronously on the main
thread** inside `regen_minimap_layers`. During a rez burst the object layer is
dirtied constantly (new prims), so that O(prims) plot loop runs on the frame
thread and spikes.

Fix (do **not** throttle the minimap's currentness — it is a primary navigation
aid, e.g. driving a vehicle fast through not-yet-rezzed areas needs it live):

1. **Move the layer rasterization off the frame thread**, mirroring the existing
   `composite_minimap` pattern: on the main thread, when a layer is dirty,
   snapshot only the inputs it needs into owned `Send` data (per-object
   `east/north/flags/scale/water-height`; per-region parcel grid + terrain
   patches), then spawn a task per layer that builds the `LayerRaster`; poll and
   publish the `Arc` (setting `last_stamp = None` to trigger a recomposite)
   when it finishes. Coalesce like the composite does (one task per layer in
   flight; re-dirty coalesces into the next spawn).
2. **Stationary throttle** (complementary, cheap): skip the
   object/terrain/parcel rebuild when the avatar position **and** camera angle
   are unchanged since the last rebuild **and** no new region connections have
   appeared **and** the layer is not otherwise dirtied — the object layer is not
   crucial while the avatar is stationary. This must never suppress a rebuild
   caused by real movement, a new neighbour region, or an explicit dirty flag.

Deferred out of the 2026-07-30 UI-dirtying session to avoid mixing concerns.
Verify with a ≤10 s `tracy-grab.sh` capture during active rez: the
`regen_minimap_layers` max/std must drop to near its mean (no main-thread
spikes), and the minimap must still update live while moving.
