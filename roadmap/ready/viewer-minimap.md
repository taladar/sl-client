---
id: viewer-minimap
title: Minimap (net map) — floater, surface, zoom, rotation, frustum
topic: viewer
status: ready
origin: user request (2026-07); fleshed out from Firestorm research 2026-07-22
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-beacons-control, viewer-avatar-radar, viewer-world-map-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The net map: the small top-down view of the region around you. This base
task is the floater + map surface + camera-ish affordances; the content
layers and interactions are split out:
[[viewer-minimap-object-layer]] (untextured objects),
[[viewer-minimap-parcel-overlay]] (parcel fills + property lines),
[[viewer-minimap-avatar-dots]] (dots, hover, chat rings),
[[viewer-minimap-interactions]] (clicks, double-click teleport, context
menu), and later [[viewer-avatar-radar]].

Reference facts (Firestorm, researched 2026-07-22; `llnetmap.cpp`,
`llfloatermap.cpp`, `floater_map.xml`):

## Floater

- Chrome-less floater (`title=""`, `header_height=0`, `setIsChrome`),
  resizable, default 200×200, min 64×64, `save_rect` +
  `save_visibility`, single instance. Minimized shows a "Mini-map"
  caption; double-click restores.
- Firestorm pins opacity to one setting regardless of focus:
  `FSMiniMapOpacity`, default 0.66.
- Eight compass labels (N/E/S/W + diagonals, white at 0.7 alpha)
  repositioned every frame by projecting their angle onto the rect edge;
  diagonals hide when the box would cover > `0.07 × min(w,h)`
  (`MAP_MINOR_DIR_THRESHOLD`). Labels rotate with the map.
- Mouse-transparent while in mouselook.
- Vintage note (2026-07-22, user-corrected): in the Vintage skin the
  minimap is a free-floating window (larger minimum size, minimize
  disabled, different default spawn position — not docked). Default
  spawn top-right, minimize disabled to match.

## Map surface & transforms

- One scale value, `mScale` = **pixels per 256 m region**, persisted as
  `MiniMapScale` (default 128), clamped 32–4096, shared across all
  minimap instances. `pixels_per_meter = scale / 256`.
- `globalPosToView` / `viewPosToGlobal` world↔widget transforms carry
  the rotation; everything (tiles, layers, dots) goes through them.
- Panning: SHIFT-drag pans (`mCurPan` offset, cursor hidden and
  recentered, 2 px drag slop); with `MiniMapAutoCenter` (default on)
  the pan lerps back to centre each frame (interpolant 0.1, snap under
  0.5 px). A "re-center" action zeroes it on demand.

## Terrain backdrop (shared data with the world map)

Per-region quads from the region list, positioned relative to the
camera position:

- On Second Life the reference uses the **sim surface composition
  texture** (`getLand().getSTexture()`) — i.e. the same terrain data we
  already hold in the scene mirror; we can composite our own terrain
  texture rather than fetching anything.
- On OpenSim it uses the region's **world-map tiles**
  (`getWorldMapTiles()`, with var-region sub-tiling) — the same imagery
  [[viewer-world-map-floater]] fetches, so this path must reuse that
  task's tile fetch/cache (`sl-map-tools`), not add a second fetcher
  (user request: same data, one source).
- Tint: current region white, other regions `0.8` grey, dead/unreachable
  regions `(1, 0.5, 0.5)`.

## Zoom

- Scroll wheel: `scale ×= 1.04^-clicks` (4% per notch), zooming toward
  the cursor when auto-center is off.
- Presets (used by the context menu): very close = 1024, close = 256,
  medium = 128 (default), far = 32 pixels/region.

## Rotation

- `MiniMapRotate` (default on = camera-at-top; the reference forces it
  to north-up once for brand-new users). Rotation angle
  `atan2(camera_at.x, camera_at.y)` applied to the whole map around its
  centre and baked into both transforms; compass labels follow.

## Camera frustum wedge

- A translucent wedge from the self position: angular width = horizontal
  FOV × aspect, radius = far-clip distance in map pixels, colour
  `MapFrustumColor` (white at 0.1 alpha). With rotate-on the map turns
  under a fixed wedge; with north-up the wedge itself rotates by camera
  yaw. Own marker details live in [[viewer-minimap-avatar-dots]].

## Update cadence

The surface (tiles, quads, transforms) draws every frame; the content
layers are cached textures regenerated on their own triggers (0.5 s
timer for objects, 3 m centre move for parcels — see the layer tasks).

Builds on: `CoarseLocationUpdate` handling in `avatars.rs` (incl. the
`viewer-r24` per-region fix), the scene mirror (terrain), and
`sl-map-tools` tiles via [[viewer-world-map-floater]].

Deps: [[viewer-ui-widget-scaffold]] (the panel / floater).
