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
The data source is the collision bitmap the simulator sends when the agent
nears a parcel it may not enter: an unsolicited `ParcelProperties` whose
sequence id is one of the ban-line ids (`-20000` not in group, `-30000`
banned, `-40000` not on the access list) and whose `bitmap` marks the blocked
4 m cells; the reference keeps it per region with a timed decay
(`LLViewerParcelMgr::mCollisionBitmap`).

The decode half is done (audit 2026-10-15, commit 4f6d765d): the record
reaches the runtimes as an `Event::ParcelProperties` and
`ParcelInfo::collision()` classifies it as a `ParcelCollision`
(`sl-proto/src/types/parcel.rs`). Still needed: a per-region collision bitmap
resource with decay fed from those records, hooked into the minimap
parcel-layer regeneration (`sl-viewer-map/src/minimap.rs`, whose module docs
still note the fill has no data source; the layer refresh triggers are in
place), and the `MiniMapCollisionParcels` setting.
Related in-world rendering: [[viewer-parcel-ban-line-display]].
