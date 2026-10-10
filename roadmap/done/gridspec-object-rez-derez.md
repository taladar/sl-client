---
id: gridspec-object-rez-derez
title: Rez, take, take-copy, delete, return and auto-return on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-object-properties,
  test-conformance-object-asset-format-fails-under-load,
  server-fake-grid-object-return, gridspec-aditi-test-land,
  gridspec-inventory-offers, gridspec-task-inventory]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-10)

Measured and written up in `book/src/gridspec/building.md`.

- **Discover.** `object-rez-derez` rewritten as a census of one avatar and
  its own objects: a copy, a take, a Restore to Last Position, a rez of the
  taken item and the record of what it rezzed, an `ObjectDelete`, a delete
  naming the Trash and one naming another folder, a return, a linkset, and
  three temporary prims. A new `object-rez-land` measures what land has to
  say: a rez where the avatar may not build, an owner returning somebody's
  object, and a parcel's auto-return. Each ran four and three times on
  aditi (Mauve) and on OpenSim; on aditi `object-rez-land` finds a no-build
  parcel in its login region by itself and runs with one avatar.
- **Findings.** Second Life announces a taken item with the **legacy**
  `UpdateCreateInventoryItem`, as OpenSim does, and not with a
  `BulkUpdateInventory`: the fake grid, its docs and two cases said
  otherwise on reasoning nobody had measured. No derez sends a `DeRezAck`.
  Second Life kills a derezzed linkset in one message naming every prim,
  OpenSim in two naming the root alone. Second Life files a delete into the
  folder the derez names, OpenSim into the agent's Trash whatever it named.
  `ObjectDelete` deletes without a trace on Second Life and does nothing on
  OpenSim. Both file a return of the agent's own object in its Lost And
  Found and tell of it in an instant message — Second Life at once,
  OpenSim with its next backup. A rezzed object's record keeps the creation
  date and masks of the object its item was made from, names the item, and
  names the agent as last owner; only Second Life's names the folder. A
  temporary prim stands a minute or a little more on both. A
  `RezRestoreToWorld` puts the object back where it stood on Second Life and
  does nothing on OpenSim, which has no listener for it. An `ObjectAdd`
  on no-build land is refused with an alert on both; a rez out of the
  inventory with an alert on Second Life and without a word on OpenSim.
- **Fake grid.** `RezPolicy` (`imitates.rs`, ten rows) and a rez refusal on land
  that does not let the agent build. For both flavours: the take's announcement
  is the legacy one and never under the derez's transaction, a return of the
  agent's own object is filed and told of, a taken item carries the object's
  masks, and a rezzed prim's record its creation date and last owner. Both cases
  run offline on both flavours, held to a table of some ninety measured answers.
- **Viewer.** New `e2e_objects` tests: on each fake flavour and on each live
  grid a prim rezzed and named through the Build window is taken from its
  pie, its item shows in the inventory window, a drag of the item puts an
  object of that name back, and Delete takes it away. The viewer waits for
  no announcement — the session keeps what either form carries. Nothing of
  the viewer's needed changing. On the live OpenSim the test fails now and
  then in two ways not understood
  ([[test-e2e-objects-live-opensim-intermittent]]); a drop and a right
  click that do nothing now say why at debug.
- **Also fixed.** `PrimBlock::to_properties` (`sl-object-asset`) carries the
  asset's `birthtime` as the record's creation date, and `from_object`
  writes it. The harness gained `GridTest::accounts_on`, for a case whose
  second avatar has nothing to do on one grid. A fake-grid test that still
  waited for a return's `DeRezAck` sat for two hours instead of failing:
  every event-wait loop in `sl-fake-grid/tests/` now has one deadline for
  the whole wait, nextest ends a default-profile test after half an hour,
  and what an audit found elsewhere is
  [[test-unbounded-waits-in-test-harnesses]].
- **Not done here.** The fake grid returns nobody else's object, returns
  nothing by itself and ends no temporary prim
  ([[server-fake-grid-object-return]]). Not measured: a return by somebody
  else and an auto-return on Second Life ([[gridspec-aditi-test-land]]); a
  rez of an item its owner may not copy, which takes two avatars and a
  hand-over ([[gridspec-inventory-offers]]) — so the viewer's no-copy
  Restore guard still rests on the reference viewer's word; who else in the
  region is sent a kill; the save destinations
  ([[gridspec-task-inventory]]).

## Known already

`object_assets` / `inventory_announcement` rows exist. On stock OpenSim
`ObjectDelete` is a no-op and a Delete derez resolves the caller's Trash; the
fake grid honours `ObjectDelete` on both flavours.

An object's creation date is in **microseconds** on both grids
([[gridspec-object-properties]]). `PrimBlock::to_properties`
(`sl-object-asset`) leaves it zero for a prim rezzed from an asset, on the
reasoning that the asset's `birthtime` is in microseconds and the record's
date in seconds — the second half of which was wrong. What a rezzed object's
record says of its creation date, item, folder and last owner is this task's
to measure; `object-properties` read only a prim made by `ObjectAdd`.

## Discover

Run `object-rez-derez` on aditi (sandbox debris!); record `ObjectDelete`,
take-copy, no-rez land refusal, temp-on-rez timing.

## Document

`book/src/gridspec/building.md` § Rez and take.

## Fake grid

Small — `ObjectDelete` flavour row, rez refusal.

## Viewer

Take waits for either announcement; the no-copy Restore guard.
