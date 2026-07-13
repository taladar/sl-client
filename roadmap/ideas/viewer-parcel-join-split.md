---
id: viewer-parcel-join-split
title: Parcel join / split
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-framework, viewer-input-system]
---

Context: [context/viewer.md](../context/viewer.md).

Drag a land selection on the ground and **subdivide** or **join** parcels,
respecting ownership / permissions.

Reference (Firestorm, read-only): `llviewerparcelmgr`,
`llviewerparcelselection`; messages `ParcelDivide`, `ParcelJoin`.

Builds on: `protocol-13` parcel and the parcel-overlay data.

Deps: [[viewer-ui-framework]], [[viewer-input-system]] (land drag-select).
