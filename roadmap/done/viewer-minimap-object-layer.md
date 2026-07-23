---
id: viewer-minimap-object-layer
title: Minimap object layer — untextured objects, ownership & depth colours
topic: viewer
status: done
origin: user request (2026-07-22); split from viewer-minimap
blocked_by: [viewer-minimap]
refs: [viewer-minimap-parcel-overlay]
---

Context: [context/viewer.md](../context/viewer.md).

The "untextured objects" raster on the minimap: prims drawn as solid
colour blobs into a cached texture composited over the terrain backdrop.
Reference facts (Firestorm `llviewerobjectlist.cpp
renderObjectsForMap`, `llnetmap.cpp renderPoint`/`createImage`,
researched 2026-07-22):

## Which objects are in the layer

Membership is maintained as objects stream in (`addToMap` /
`removeFromMap`), not queried per repaint. Included: volume objects
that are **owned by you** OR **large** (scale magnitude > 7.5 m) — the
map does NOT show every prim. Firestorm adds opt-in classes, each with
a toggle: physical (`FSNetMapPhysical`), scripted (`FSNetMapScripted`),
temp-on-rez (`FSNetMapTempOnRez`) — all default off. Excluded always:
dead, orphaned, regionless objects and **attachments**. Master toggle
`MiniMapObjects` (default on).

## Rasterization

- Each object is a filled square: `radius ≈ (scale.x + scale.y) × 0.25
  × 1.3` (fudge), clamped to `MiniMapPrimMaxRadius` (default 16 m);
  owned/accented objects get a 2 m minimum radius so your small stuff
  stays visible.
- Vertical cull: skip objects with `|z − agent.z|` >
  `MiniMapPrimMaxVertDistance` (default 256 m).
- Target image: power-of-two square, 64–512 px, sized to the widget
  diagonal; `texels_per_meter = image_width / (diagonal / scale × 256)`.

## Colours (ownership × above/below water)

The above/below split is relative to the **region water height** (not
the camera; avatar dots handle the camera-relative cue):

- others above water `NetMapOtherOwnAboveWater` (dark grey 0.24), below
  `NetMapOtherOwnBelowWater` (darker 0.125);
- yours above `NetMapYouOwnAboveWater` (cyan 0/1/1), below
  `NetMapYouOwnBelowWater` (0/0.78/0.78);
- group-owned above `NetMapGroupOwnAboveWater` (magenta), below
  `NetMapGroupOwnBelowWater` (0.78/0/0.78).

Firestorm accent overrides (in order, when their toggle is on):
scripted → `NetMapScripted` (orange); physical →
`NetMapYouPhysical`/`NetMapGroupPhysical`/`NetMapOtherPhysical`;
temp-on-rez → `NetMapTempOnRez` (orange); phantom objects get their
alpha set from `FSNetMapPhantomOpacity`.

## Update cadence

Regenerated at most ~2 Hz: only when a dirty flag is set (scale change,
resize) or a 0.5 s timer elapses; the raster is cleared to transparent,
redrawn from the membership list, uploaded once, then drawn as a quad
every frame centred on the capture-time position. Follow this shape —
it is what keeps the layer cheap.

Our side: ownership tests come from the object permissions we already
track (`permYouOwner`/group equivalents in the scene mirror); water
height per region is in the region data. The parcel layer
([[viewer-minimap-parcel-overlay]]) is a separate raster with its own
refresh trigger — keep them independent textures as the reference does.

Reference (Firestorm, read-only): `llviewerobjectlist.cpp`
(`renderObjectsForMap`), `llnetmap.cpp` (`renderPoint`,
`renderScaledPointGlobal`, `createObjectImage`).

Deps: [[viewer-minimap]] (the surface and transforms it draws into).

## Done (2026-07-23)

Membership, colours and rasterisation in `minimap_math.rs`
(`object_on_map`, `object_map_color`, `object_map_radius`,
`render_object_point` — reference formulas incl. the 1.3 fudge, 16 m
clamp, 2 m owned/accent floor, ±256 m vertical cull) with unit tests;
the layer itself in `minimap.rs` (`regen_minimap_layers`): a pow-2
64–512 raster sized to the widget diagonal, cleared and redrawn at most
every 0.5 s (or on dirty), drawn centred on its capture-time position.
Ownership/class come from the `PrimFlags` bits already mirrored per
object (`ObjectState::minimap_objects` ORs each prim's flags with its
root's and excludes attachments); above/below water from the per-region
handshake water height. Settings: `MiniMapObjects` on;
`NetMapPhysical`/`NetMapScripted`/`NetMapTempOnRez` off;
`NetMapPhantomOpacity` 100; `MiniMapPrimMaxRadius` 16;
`MiniMapPrimMaxVertDistance` 256.
