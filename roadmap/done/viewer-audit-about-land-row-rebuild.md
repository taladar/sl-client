---
id: viewer-audit-about-land-row-rebuild
title: About Land rebuilds every owner and access row whenever any avatar name resolves
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-extract-and-test-pure-logic]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-places/src/about_land.rs:2065` (`sync_owners_view`) and `:2159`
(`sync_access_view`) both gate on `avatars.is_changed()` — which fires on
**every write to the name cache**, including a name for an avatar that appears
in neither list.

In a crowded region that is a full row rebuild, with a `translator.get()` and a
`format!` per row, many times a second while About Land is open.

Fix: gate on whether a name *this list displays* actually changed — compare the
resolved labels, or track a per-list revision — rather than on the whole
resource.

`about_land.rs` is 3309 lines with **zero tests**; several of its pure helpers
(`expiry_text`, `parcel_owner_label`, `day_cycle_summary`) are listed in
[[viewer-audit-extract-and-test-pure-logic]].

## Fixed (2026-09-13)

It was worse than the audit read it. `Res<AvatarState>::is_changed()` is not
"the name cache was written" — `AvatarState` holds every avatar's objects,
coarse dots, attachment nodes, appearances and complexity, so the tick is set by
anyone walking past. The rows were rebuilt on most frames of a busy region, not
merely on each name reply.

Two gates now, because the audit's own case needs both.

**A revision for the names themselves.** `AvatarState` gained
`names_revision()`, bumped only where the name cache is written (`name_entry`,
the sole record-ingest path, and `set_name_aliases`) — the same shape
`FriendsModel`, `GroupsModel` and `MuteModel` already have and that
`sl-viewer-people`'s views already rebuild against. It is an upper bound rather
than a proven change: `name_entry` hands out a `&mut` and cannot see whether the
caller writes through it, and erring that way is the cheap direction.

**A comparison of the resolved labels.** The revision alone answers "a name
resolved somewhere", which is not the audit's question — most names belong to
nobody in this list. So each view resolves its rows and only writes them when
they differ from what it already shows. Writing is what marks the view changed
and re-binds every row's text, so this is where the saving is.

That forced one structural change: the "last built from" record moved out of the
view components into a `ViewBuilt` beside them (`AboutLandBuilt`,
`AboutRegionBuilt`). Recording a revision inside the view would mark the view
changed, re-binding every row — the exact cost being avoided.

`NameRevisions` and `ViewBuilt` live in a new crate-private
`sl-viewer-places/src/name_revisions.rs`, because About Land was not alone:
About Region's four estate access lists and its `refresh_on_names`, and About
Landmark's `refresh_names`, had the identical gate and are converted with it.
The same `avatars.is_changed()` pattern survives in `sl-viewer-people` and
`sl-viewer-notices` (the name links, the experience floaters, the group
profile); those are their own tasks, and the facility they need now exists.

The premise in the last paragraph above has also expired: `about_land.rs` has
had tests since the keyed-floater work, and `expiry_text` and friends are still
worth extracting but no longer sit in an untested file.

## How it was verified

- `sl-viewer-world-api` — `the_name_revision_moves_for_names_and_nothing_else`:
  an avatar arriving in coarse view does not move it, a name learned from
  traffic does, and so does an alias replacing one.
- `sl-viewer-places/src/name_revisions.rs` — `a_reading_is_due_once` and
  `advancing_reports_only_a_move` pin the gate itself.
- `sl-viewer-places/src/about_land.rs` —
  `only_a_name_this_table_shows_rebuilds_it`, a headless app test over the
  audit's own case. It asserts on the `OwnersView` component's change tick,
  because the rebuild is invisible in the rows: an avatar moving into view does
  not write the view, a **stranger's** name resolving does not write it, and the
  listed owner's own name does — and lands in the row. The first step pins the
  revision gate, the second pins the label comparison.
