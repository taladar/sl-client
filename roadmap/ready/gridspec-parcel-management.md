---
id: gridspec-parcel-management
title: Parcel edits, divide/join, object owners and return on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, parcel-properties-update-via-udp-poisons-opensim,
  gridspec-aditi-test-land]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

UDP `ParcelPropertiesUpdate` with a null `MediaType` poisons OpenSim; divide
/ join green on OpenSim (no reply, overlay re-sent); OpenSim sends
`ParcelObjectOwnersReply` over UDP (the fake grid over the event queue) and
returns by owner only. The fake grid has no divide / join and no
`ParcelPropertiesUpdate` cap.

## Discover

`parcel-edit`, `parcel-divide-join`, `parcel-object-owners` on OpenSim and
(owned land) aditi; the update cap's response shape.

## Document

`book/src/gridspec/land.md` § Managing a parcel.

## Fake grid

Small in this task: the update cap, owners-reply transport, owner-only return,
the poisoning teeth test. Divide / join:
[[server-fake-grid-parcel-divide-join]].

## Viewer

Cap first, UDP fallback; re-request after unanswered divide / join.

## Capabilities done in this task

[[protocol-cap-parcel-properties-update]]: parcel edits over
`ParcelPropertiesUpdate`, measured against the UDP path on both grids — it
also fixes [[parcel-properties-update-via-udp-poisons-opensim]], so this task
is the first of the capability work.
