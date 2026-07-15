---
id: viewer-parcel-borders
title: In-world parcel borders (property lines)
topic: viewer
status: ideas
origin: user request (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The banded vertical property lines the reference viewer draws along parcel
boundaries in-world, colour-coded by ownership (your land, group land, someone
else's, for sale, auction, public), toggled with the show-property-lines
setting.

The data arrives already: `Event::ParcelOverlay(ParcelOverlayInfo)` carries a
`sequence_id` (0–3) and raw packed `data: Vec<u8>` — the four chunks of the
region's 64×64 parcel grid, one byte per 4 m square, packing the ownership
colour in the low bits and the `WEST_LINE` / `SOUTH_LINE` / `SOUND_LOCAL` flags
in the high bits. **Nothing decodes it and nothing in the viewer consumes it.**

So this task owns two things:

- **The decode.** Turn the four raw chunks into a typed 64×64 grid: per-cell
  ownership class plus the border and sound-local bits, reassembled across the
  four sequence chunks and invalidated on region change. Decide where it
  belongs — `sl-proto` (preferred: the packing is protocol, and two other tasks
  want the same grid — [[viewer-minimap-parcel-overlay]] for the map colours and
  [[viewer-in-world-sounds]] for the `SOUND_LOCAL` bit that clamps sound to the
  parcel) or the viewer.
- **The rendering.** Build boundary geometry from the west/south edge bits,
  drape it over the terrain heightfield (`terrain.rs` / `ground.rs` already own
  the heights), colour by ownership class, and reproduce the characteristic
  vertical banding that fades with distance. Handle multi-region: the overlay is
  per-region, and neighbour regions are already streamed.

Also needs the parcel data itself to be requested / kept current on parcel and
region change (the parcel protocol is done — see `protocol-24` and the
`ParcelProperties` work; the viewer just never asks).

Reference (Firestorm, read-only): `llviewerparceloverlay`
(`renderPropertyLines`, `updateOverlayTexture`), `llviewerparcelmgr`.

Builds on: `Event::ParcelOverlay`, the terrain heightfield, and the existing
parcel protocol.
