---
id: gridspec-parcel-access-and-ban-lines
title: Parcel access and ban lists, enforcement and ban lines on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-parcel-ban-line-display,
  viewer-parcel-ban-duration, gridspec-aditi-test-land,
  gridspec-sl-ban-line-trigger, viewer-parcel-ban-lines-on-refusal,
  viewer-about-land-list-save-leaves-flags-stale,
  server-fake-grid-parcel-access-enforcement]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Green on OpenSim (fresh transaction id per update, nil placeholder for empty);
enforcement and ban-line pushes are read from OpenSim source only; nothing
enforced on the fake grid.

## Discover

`parcel-access-list` plus a second avatar walking into a banned parcel (OpenSim
as estate owner); on aditi needs owned land.

## Document

`book/src/gridspec/land.md` § Access.

## Fake grid

Small list handling in this task; enforcement —
[[server-fake-grid-parcel-access-enforcement]].

## Viewer

Ban lines drawn, overlay redrawn, multi-part lists.

## Done (2026-10-05)

- **Discover.** Three cases. `parcel-access-list` now records the lists as
  data — who is answered, in how many packets, what an entry and an empty
  list carry, what a save does to the parcel's flags, what a refusal looks
  like — on aditi as a resident without land, on OpenSim as the estate owner
  and as a resident, and on both fake flavours. `parcel-ban-enforcement` (new,
  two avatars, OpenSim) bans a resident with no estate rights from a
  divided-off plot and records the flight in, the teleport in, the ban on an
  avatar already inside, and the allow list. `parcel-ban-line` (new, one
  avatar) sweeps a region's parcels by their bitmaps for one that is closed to
  strangers and walks into it; on aditi it met a 64 m² parcel flagged for both
  list and group.
- **Measured** (`book/src/gridspec/land.md` § Access). Both grids answer a
  stranger's list request. An empty list's placeholder carries `Flags` 0 on
  Second Life and the list's bit on OpenSim. OpenSim stamps every entry with
  its list's bit, splits a 60-entry reply 48 + 12, answers an update with
  nothing, and **switches the parcel's `USE_ACCESS_LIST` / `USE_BAN_LIST` flag
  on when a list is saved and off when it is emptied**. Second Life refuses a
  stranger's update with a plain alert; OpenSim drops it silently. A refused
  entry is an `AlertMessage` on both: a notification's *name* behind
  `NOTIFY:` on Second Life, a sentence on OpenSim. Second Life stops a
  walking avatar at the line and lifts a flying one over its top; OpenSim lets
  either a step in and puts it back. The ban line is a full `ParcelProperties`
  under `-20000` / `-30000` / `-40000`. `USE_ACCESS_GROUP` alone closes
  nothing on either grid.
- **Client.** A list longer than 48 entries goes out in sections, as the
  reference viewer's does (it was one message of any size).
  `ParcelInfo::collision()` reads the three ban-line sequence ids as a
  `ParcelCollision`. `RUST_LOG=sl_proto::wire=trace` logs every inbound UDP
  message whole, for the next probe.
- **Fake grid.** `ParcelPolicy::list_update_sets_use_flag` and
  `empty_list_placeholder_names_its_list`; a long reply is split at 48;
  `parcel-access-list` runs offline on both flavours. Enforcement stays
  [[server-fake-grid-parcel-access-enforcement]].
- **Viewer.** A plain alert that begins `NOTIFY:` or `ALERT:` is raised as
  the catalogue notification it names (the reference's `process_alert_core`);
  Second Life's parcel refusals were shown as the raw string. About Land
  already unions a multi-packet list and reads "anyone can visit" off
  `USE_ACCESS_LIST`, both unit-tested.
- **Harness.** `support::steer_towards` — the walk helper with a time budget,
  a gait (flying, flying level, walking) and "did not arrive" as an answer;
  a `ban_line_regions` fixture for the regions `parcel-ban-line` may visit.

## Not done, and why

- **When Second Life pushes a ban line is not pinned down**
  ([[gridspec-sl-ban-line-trigger]]): four records on two of four logins
  beside the closed parcel, none on a dozen approaches. `parcel-ban-line`
  records them and holds the grid to the refusal only.
- **Everything that needs land on Second Life** ([[gridspec-aditi-test-land]]):
  an accepted update, an entry's flags and expiry as stored, a long list's
  packets, and all of a *ban* — its line, its height, its `-30000` push.
- **The viewer does not draw the fence**
  ([[viewer-parcel-ban-lines-on-refusal]]), and no `e2e` test was added: the
  list handling that differs by flavour is covered by `parcel-access-list`
  offline and by unit tests, and the one viewer-visible divergence found is a
  bug of its own ([[viewer-about-land-list-save-leaves-flags-stale]]).
