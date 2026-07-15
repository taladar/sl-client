---
id: viewer-parcel-overlay-decode
title: Decode ParcelOverlay into a 64×64 grid resource
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-parcel-borders
---

Context: [context/viewer.md](../context/viewer.md).

`Event::ParcelOverlay(ParcelOverlayInfo)` carries a `sequence_id` (0–3) and raw
packed `data: Vec<u8>` — the four chunks of the region's 64×64 parcel grid, one
byte per 4 m square, packing the ownership colour in the low bits and the
`WEST_LINE` / `SOUTH_LINE` / `SOUND_LOCAL` flags in the high bits. **Nothing
decodes it and nothing in the viewer consumes it today.**

Turn the four raw chunks into a typed 64×64 grid: per-cell ownership class plus
the border and sound-local bits, reassembled across the four sequence chunks and
invalidated on region change, exposed as an ECS resource. Decide where the
decode belongs — `sl-proto` is preferred (the packing is protocol, and two other
tasks want the same grid: [[viewer-minimap-parcel-overlay]] for the map colours
and [[viewer-in-world-sounds]] for the `SOUND_LOCAL` bit that clamps sound to
the parcel) — with the viewer holding the assembled per-region resource.

Also request / keep the parcel overlay current on parcel and region change (the
parcel protocol is done — see `protocol-24` and the `ParcelProperties` work; the
viewer just never asks).

Reference (Firestorm, read-only): `llviewerparceloverlay`
(`updateOverlayTexture`), `llviewerparcelmgr`.

Builds on: `Event::ParcelOverlay` and the existing parcel protocol.
