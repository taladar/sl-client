---
id: viewer-minimap-parcel-overlay
title: Parcel borders on the minimap
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-minimap, viewer-parcel-borders]
---

Context: [context/viewer.md](../context/viewer.md).

The parcel layer on the minimap: each 4 m cell tinted by its ownership class
(yours, group, other, for sale, auction, public) with the parcel boundary lines
drawn along the west / south edges — the view that makes land ownership legible
at a glance, and the one you actually use when land-hunting.

It consumes the decoded 64×64 overlay grid that [[viewer-parcel-borders]]
produces (the `Event::ParcelOverlay` chunks turned into typed per-cell ownership
classes and edge bits), so no protocol work is needed here — this is the
map-space composite: raster the grid into the minimap's backdrop under the
avatar dots, keep it aligned across neighbour regions as they stream in and out,
invalidate it when the overlay is re-sent (parcels get split, joined and sold
while you watch), and put it behind its own toggle.

Reference (Firestorm, read-only): `llnetmap` (parcel overlay drawing),
`llviewerparceloverlay`.

Deps: [[viewer-minimap]] (the surface it draws on),
[[viewer-parcel-borders]] (the decoded overlay grid).
