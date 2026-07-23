---
id: viewer-minimap-collision-parcels
title: Minimap collision-parcel ("banned from here") fill
topic: viewer
status: ideas
origin: split from viewer-minimap-parcel-overlay (2026-07-23)
blocked_by: []
refs: [viewer-minimap-parcel-overlay, viewer-parcel-ban-line-display]
---

Context: [context/viewer.md](../context/viewer.md).

The one parcel-layer fill the minimap port skipped: the reference
tints the parcel cells you were just refused entry to (pale red
`(255,128,128,192)`, setting `MiniMapCollisionParcels`, default on).
The data source is the collision bitmap the simulator sends when a
movement is blocked by a ban/access line: a `ParcelProperties` with
`request_result` = the collision variants (banned / not-on-access-list)
whose `bitmap` marks the blocked 4 m cells; the reference keeps it per
region with a timed decay (`LLViewerParcelMgr::mCollisionBitmap`).

Needs: surface those collision `ParcelProperties` (we already decode
`request_result` — check the collision variants reach a viewer-readable
event), keep a per-region collision bitmap resource with decay, hook it
into the minimap parcel-layer regeneration (the layer refresh triggers
are already in place) and the `MiniMapCollisionParcels` setting.
Related in-world rendering: [[viewer-parcel-ban-line-display]].
