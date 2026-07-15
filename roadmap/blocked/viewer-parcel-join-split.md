---
id: viewer-parcel-join-split
title: Parcel join / split
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-input-action-map, viewer-parcel-overlay-decode]
---

Context: [context/viewer.md](../context/viewer.md).

Drag a land selection on the ground and **subdivide** or **join** parcels,
respecting ownership / permissions. The land drag-select uses input **actions**
([[viewer-input-action-map]]) and the parcel boundaries come from the decoded
overlay grid ([[viewer-parcel-overlay-decode]]).

Reference (Firestorm, read-only): `llviewerparcelmgr`,
`llviewerparcelselection`; messages `ParcelDivide`, `ParcelJoin`.

Builds on: `protocol-13` parcel and the parcel-overlay data.

Deps: [[viewer-input-action-map]] (land drag-select),
[[viewer-parcel-overlay-decode]] (parcel boundaries).
