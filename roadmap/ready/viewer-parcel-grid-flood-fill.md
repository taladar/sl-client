---
id: viewer-parcel-grid-flood-fill
title: Flood-fill the parcel-overlay grid into per-parcel footprints
topic: viewer
status: ready
origin: user request (2026-07); follow-up to viewer-parcel-overlay-decode
refs: [viewer-parcel-overlay-decode, viewer-minimap-parcel-overlay, viewer-parcel-borders-render]
---

Context: [context/viewer.md](../context/viewer.md).

The decoded `ParcelOverlayGrid` ([[viewer-parcel-overlay-decode]]) already
carries the two property-line bits per 4 m square (`west_line` / `south_line`),
and those bits *are* the parcel partition of the region. Add a helper on the
grid that turns them into the set of distinct parcels — their footprints — so
consumers never have to probe the region cell by cell.

The reconstruction is a connected-components pass with the line bits as walls:
square `(row, col)` is cut off from its **west** neighbour `(row, col-1)` iff
`(row, col).west_line`, and from its **south** neighbour `(row-1, col)` iff
`(row, col).south_line`; every connected component is one parcel's footprint.
That yields, from the single region-entry overlay push alone: the **number** of
parcels, each one's exact **footprint** (the squares it owns, hence a
representative square and a bounding rectangle), and its per-square ownership
colour — with **no** `ParcelProperties` traffic.

Scope: a pure method on `ParcelOverlayGrid` in `sl-proto` (e.g.
`parcels() -> Vec<ParcelFootprint>` with each footprint exposing its squares, a
representative square, and a region-local bounding rectangle), fully unit-tested
on synthetic grids (single parcel, quartered region, L-shaped parcel, region
edges as implicit walls). Keep it pure — no ECS, no I/O.

Why it matters: this is the **enumeration primitive** the region-wide parcel
sweep needs. Knowing the footprints, the sweep sends exactly one
`ParcelPropertiesRequest` per parcel (a rectangle over a representative square)
to fetch each parcel's `local_id`, owner and flags — N requests for N parcels,
never a 64×64 probe. An LSL script, with no overlay access, is forced into that
brute-force sampling; a viewer client is not. Both
[[viewer-minimap-parcel-overlay]] (per-parcel tints) and any region-wide
parcel-flag/ban work build on this.

Reference (Firestorm, read-only): `llviewerparceloverlay.cpp`
(`updatePropertyLines`, which walks the same `PARCEL_WEST_LINE` /
`PARCEL_SOUTH_LINE` bits to draw boundaries).

Builds on: [[viewer-parcel-overlay-decode]] (the `ParcelOverlayGrid` and its
`west_line` / `south_line` cell fields).
