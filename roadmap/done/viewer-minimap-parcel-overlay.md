---
id: viewer-minimap-parcel-overlay
title: Parcel fills & property lines on the minimap
topic: viewer
status: done
origin: user request (2026-07); fleshed out from Firestorm research 2026-07-22
blocked_by: [viewer-minimap, viewer-parcel-overlay-decode]
refs: [viewer-minimap-object-layer]
---

Context: [context/viewer.md](../context/viewer.md).

The parcel layer on the minimap: for-sale/auction fills and the
west/south property lines — the view that makes land ownership legible
when land-hunting. It consumes the decoded per-cell grid
[[viewer-parcel-overlay-decode]] produces (done): 4 m cells, low bits =
ownership class (public 0, owned 1, group 2, self 3, for-sale 4,
auction 5), high bits = `WEST_LINE` (0x40) / `SOUTH_LINE` (0x80).

Reference facts (Firestorm, researched 2026-07-22 — note the minimap
does **not** reuse the 3-D world parcel-overlay texture; it has its own
raster, the Catznip "World-MinimapOverlay" patch,
`llnetmap.cpp:1471-1568` `renderPropertyLinesForRegion`):

- Own cached texture (`mParcelImagep`), same 64–512 px pow-2 sizing as
  the object layer, drawn as its own quad above the object layer.
- Per region: the region's north edge row and east edge column are
  drawn as border lines; then each 4 m cell draws
  - a fill when (`MiniMapForSaleParcels` && for-sale/auction) or
    (`MiniMapCollisionParcels` && the cell is in the collision bitmap —
    the "banned from here" highlight after a blocked entry):
    for-sale `(255,255,128,192)` pale yellow, auction
    `(128,0,255,102)` violet, collision `(255,128,128,192)` pale red;
  - its `SOUTH_LINE` bit as a horizontal line and `WEST_LINE` bit as a
    vertical line, in `MapParcelOutlineColor` (white); corners need no
    special casing — they are the union of the two edge writes.
- Dead/unreachable regions draw their lines in `(255,128,128,255)`.
- Settings (all default on): `MiniMapShowPropertyLines` (master gate —
  also gates the parcel part of the hover tooltip, see
  [[viewer-minimap-avatar-dots]]), `MiniMapForSaleParcels`,
  `MiniMapCollisionParcels`.
- Regeneration trigger: dirty flag OR the map centre moved **> 3 m**
  (squared-distance test) — unlike the object layer's 0.5 s timer. Also
  invalidated by parcel-overlay update and collision callbacks (parcels
  split/join/sell while you watch; keep our equivalent hooked to the
  overlay-decode resource's change detection).

The general ownership tint classes (self green, group teal, other red,
public transparent, `PropertyColor*` in the reference) belong to the
**world/3-D** overlay, not the minimap raster — the minimap only fills
for-sale/auction/collision and draws lines. Keep that split unless we
deliberately choose to do better; if we add an ownership-tint option it
should default off to match the reference look.

Reference (Firestorm, read-only): `llnetmap.cpp`
(`renderPropertyLinesForRegion`, layer compositing in `draw()`),
`llviewerparceloverlay.cpp` (the 3-D overlay + colour tables, for
contrast), `llviewerparcelmgr` (collision bitmap).

Deps: [[viewer-minimap]] (the surface),
[[viewer-parcel-overlay-decode]] (the decoded grid — done).

## Done (2026-07-23)

`render_parcel_region` in `minimap_math.rs` (unit-tested port of the
Catznip rasteriser: north/east border lines, per-4 m-cell for-sale
pale-yellow / auction violet fills, south/west property lines in white)
drawn into its own 64–512 pow-2 raster in `minimap.rs`, regenerated on
overlay change-detection (`SlParcelOverlay` `is_changed`), toggle
changes, or a >3 m centre move — not the object layer's timer.
Settings `MiniMapShowPropertyLines` / `MiniMapForSaleParcels` (both on;
the property-lines master also gates the tooltip's parcel section).

Follow-up (same day): overlay chunks are now tagged with their source
region in `sl-proto` (root *and* child circuits — the child dispatcher
previously dropped them) and `SlParcelOverlay` holds one grid **per
region**, so neighbour regions draw their own property lines once
their circuit delivers an overlay (Second Life pushes it on child
establishment; OpenSim only on parcel changes — until then a neighbour
draws its full four-edge border outline instead of just north/east).

Gaps, pending data sources: the collision ("banned from here") fill
has no client-side bitmap yet — split out to
[[viewer-minimap-collision-parcels]]; dead-region red lines need a
region-liveness signal we do not track.
