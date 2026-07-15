---
id: viewer-parcel-borders-render
title: In-world parcel borders (property lines)
topic: viewer
status: ready
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
