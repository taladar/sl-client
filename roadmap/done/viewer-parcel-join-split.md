---
id: viewer-parcel-join-split
title: Parcel join / split
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-input-action-map, viewer-parcel-overlay-decode]
---

Done (2026-09-19): part of the Land tool (`sl-viewer-edit/src/edit_land.rs`,
[[viewer-terrain-edit-brushes]]). A land drag-select builds a rectangle snapped
out to the 4 m parcel grid (the reference's `LLToolSelectLand::handleMouseUp`
arithmetic) and asks for its `ParcelProperties`; a plain click asks with
`snap_selection` so the simulator snaps the selection to the whole parcel.

**Subdivide** and **Join** reproduce `LLViewerParcelMgr::startDivideLand` /
`startJoinLand` refusal for refusal --
`CannotDivideLandNothingSelected` / `CannotDivideLandPartialSelection`,
`CannotJoinLandNothingSelected` / `CannotJoinLandEntireParcelSelected` /
`CannotJoinLandSelection` -- then raise `LandDivideWarning` / `JoinLandWarning`
and send `ParcelDivide` / `ParcelJoin` only on the OK answer, against the
rectangle the warning was raised for rather than whatever is selected when the
answer arrives. Both buttons grey out on the same gate they refuse on.

**Not verified live**: needs land the test avatar may divide.

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
