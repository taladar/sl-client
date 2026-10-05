---
id: gridspec-parcel-management
title: Parcel edits, divide/join, object owners and return on each grid
topic: gridspec
status: done
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

## Done (2026-10-05)

Measured on aditi and OpenSim, written up in `book/src/gridspec/land.md`, and
held to the measurement with `Measured` constants:

- `parcel-properties`: both grids send `MediaData` and the visibility flags
  over the event queue; only Second Life sends `ParcelExtendedFlags`; neither
  sends `MediaLinkSharing` for a parcel without media.
- `parcel-edit`: the edit goes over the capability on both grids; OpenSim
  ignores `ParcelPropertiesRequestByID` and echoes the edit under the client's
  last request's sequence id; `MediaType` stays set in OpenSim's database.
- `parcel-edit-refused` (new, primary avatar, live only): neither grid fails
  the POST or alerts; Second Life pushes the unchanged parcel back under
  `-10000` and answers by-id requests, OpenSim sends nothing.
- `parcel-divide-join` green on OpenSim. `parcel-object-owners` is now
  two-avatar: OpenSim returns by owner only, so the returned cube belongs to a
  dedicated local resident account (`--secondary resident`) that owns nothing.

Fake grid: `ImitatedGrid::parcel_policy` (by-id answered, echo sequence, UDP
media-type wipe, unset media block and extended blocks, task-list returns,
owners-reply transport), parcel records over the event queue (the UDP form
dropped the media blocks), return types honoured, and the OpenSim teeth test
`an_about_land_save_keeps_the_media_type_on_opensim`. The two LLSD parcel
encoders now share the CAPS-only fields.

Viewer: About Land sends no by-id read-back after Apply (the update's push
replaces it); the land tool re-requests its selection after a divide or join.

Not measured on Second Life, for want of land
([[gridspec-aditi-test-land]]): an accepted edit, divide / join, the owners
reply and returns.
