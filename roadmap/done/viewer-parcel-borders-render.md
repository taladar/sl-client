---
id: viewer-parcel-borders-render
title: In-world parcel borders (property lines)
topic: viewer
status: done
origin: user request (2026-07); split from viewer-parcel-borders
blocked_by: [viewer-parcel-overlay-decode]
---

Context: [context/viewer.md](../context/viewer.md).

Draw the banded vertical property lines the reference viewer shows along parcel
boundaries in-world, colour-coded by ownership (your land, group land, someone
else's, for sale, auction, public), toggled with the show-property-lines
setting.

Consume the typed 64×64 grid from [[viewer-parcel-overlay-decode]]: build
boundary geometry from the west/south edge bits, drape it over the terrain
heightfield (`terrain.rs` / `ground.rs` already own the heights), colour by
ownership class, and reproduce the characteristic vertical banding that fades
with distance. Handle multi-region: the overlay is per-region, and neighbour
regions are already streamed.

Reference (Firestorm, read-only): `llviewerparceloverlay`
(`renderPropertyLines`), `llviewerparcelmgr`.

Builds on: the parcel-overlay grid resource and the terrain heightfield.

## Done (2026-08-02)

New viewer module `parcel_borders.rs` + shader `parcel_borders.wgsl`.
Consumes `SlParcelOverlay` (one `ParcelOverlayGrid` per region) and drapes a
band mesh over `TerrainState::land_height` along every parcel boundary. The
edge derivation is the reference `renderPropertyLines` set — a coloured
square draws its own `west_line` / `south_line` plus the derived east / north
edges (the region's outer rim, or where the neighbouring square carries the
shared boundary); public / unassigned squares draw nothing. Ownership tint is
the reference `PropertyColor*` palette (self green, group teal, other red,
for-sale orange, auction violet). One band-mesh entity per region, placed at
the region's south-west corner relative to the moving scene origin (the
terrain-patch placement), so neighbours draw their own lines; rebuilt on
overlay change, terrain-height change, or an origin recenter, coalesced behind
a 0.5 s cooldown. Gated by the `ShowPropertyLines` setting (World ▸ Property
Lines menu toggle; **defaulted on** since there is no preferences UI yet —
reference default is off). Pure edge-derivation (`region_border_edges`) is
unit-tested.

Deliberate departures from the reference (user-requested):

- **Public / unowned parcel boundaries are drawn** (in grey), not skipped — so
  public land's extent is legible. The reference draws nothing for public
  (`PropertyColorAvail` is transparent).
- **The region's outer rim is always drawn in white as a sim crossing**, even
  between two public regions, so where a region crossing happens is always
  visible (a main purpose of the feature). The reference only shows the rim in
  the edge parcel's ownership colour, so a public-on-public sim boundary would
  be invisible.
- Region-rim bands sample terrain height clamped just inside the region edge
  (`land_height` returns nothing exactly on the far edge), so rim bands are not
  dropped near the corners.

Interpretation notes (differ from a literal reading of the modern reference):

- The bands are **short vertical strips** (1 m, fading to transparent at the
  top — the "vertical banding"), not the modern reference's flat ground
  ribbons: modern Firestorm `renderPropertyLines` was optimised to flat lines,
  but the requested/classic look is a vertical band (nearest SL constant:
  `PARCEL_POST_HEIGHT` = 0.666 m, the parcel-post/collision wall height). Uses
  the ownership colours + edge logic from `renderPropertyLines`.
- **Fades with distance** in-shader: a smooth alpha ramp from 128 m to 256 m
  (the reference clips per-edge at 256 m, `PROPERTY_LINE_CLIP_DIST`), using the
  view bind group's camera position — no per-frame CPU work, no per-edge LOD.
- A shared boundary draws **both** parcels' colours, each band inset a hair
  (`LINE_WIDTH` = 0.0625 m) toward its own square (the reference's tick).
- A band foot is **clamped up to the region water surface** where the terrain
  is submerged (`WaterState::height_of` + 0.01 m), so a boundary crossing water
  rides on the surface rather than sinking to the seabed — the reference's
  above-water property-line behaviour. (The reference's separate *underwater*
  pass, drawn only when the camera is below the surface, is not reproduced.)
- Material is a tiny unlit, alpha-blended `ParcelBorderMaterial` (empty bind
  group; the ownership tint + band fade ride the mesh's per-vertex colour),
  modelled on `TerrainMaterial`.
