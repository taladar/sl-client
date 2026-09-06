---
id: viewer-floaters-never-reread-after-a-push
title: A floater seeds its draft once and reverts what it never saw
topic: viewer
status: done
origin: doing test-fake-grid-concurrent-edits (2026-09-05)
points: 3
refs:
  [
    test-fake-grid-concurrent-edits,
    test-asset-save-mutation-survey,
    viewer-task-inventory-open-and-save-back,
    viewer-region-push-discards-pending-edits,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

[[test-fake-grid-concurrent-edits]] gave the fake grid the pushes a simulator
sends when somebody *else* changes what you are looking at: an object's
properties to whoever holds it selected, a sequence-zero `ParcelProperties` to
the parcel's occupants, a `RegionInfo` to the region. Nothing in the viewer
consumed any of them as a re-read.

## The rule that landed

`ParcelUpdate::merge_unedited(base, fresh)` in `sl-proto`, beside
`ParcelInfo::to_update` — a three-way merge. A form field that still equals the
record it was seeded from is one nobody here has touched, so an arriving record
owns it; a field that has moved away is this resident's pending edit and is
kept. The caller then advances the base, so a field edited to the value the grid
already holds stops counting as an edit, and a repeated record is one change
rather than an endless one.

It is destructured **without** a `..` rest pattern: a field added to
`ParcelUpdate` and forgotten here is a compile error, and one added to the
destructure but forgotten in the merge is an unused binding. The hazard this
whole conversion family has is a forgotten field, so neither list may drift in
silence.

## About Land

`draft_ready: bool` is gone. `AboutLandState::seeded: Option<ParcelUpdate>`
replaces it and subsumes it — "there is a base" *is* "the draft is seeded" — and
every `ParcelProperties` for the bound parcel now merges into the draft instead
of being dropped on the floor.

**No branch on "unsolicited".** The entry asked for one; it is neither possible
nor wanted. Not possible because `apply_draft` requests its own read-back with
`sequence_id: 0`, which is exactly the value that marks an unsolicited push, so
the floater cannot tell its own echo from a foreign one. Not wanted because the
merge is the right answer to all three things that arrive on that path: a
foreign push (carry their change), the read-back after this floater's own
**Apply** (agrees with the draft, so nothing moves), and an ordinary refresh.

### The text fields needed their own answer

The six edit fields are the one part of the form **not** mirrored into the draft
until **Apply** reads them, so a resident's typing lives in the widget where the
draft's merge cannot see it. `AboutLandState::shown_fields` records what the
widgets were last written with; a widget still reading exactly that is one
nobody has typed in and may be rewritten, anything else is left alone.

The first cut compared the widget against `seeded` and was wrong: the merge has
already advanced that to the *new* record by then, so every untouched field
looked edited and would have stopped refreshing — the opposite bug, and a
quieter one. The comparison has to be against what the widget was **given**,
which is not the same thing as the merge's base.

`AboutLandDirty::seed_fields` became a `FieldSeed` (`None` / `All` /
`Unedited`) rather than a bool, because "rewrite the fields" now has two
meanings: a fresh subject, whose old text means nothing, and a merge, where the
old text is what tells the two cases apart.

## The other three surfaces the entry listed

**The build floater was already right** — the entry predicted it was not, and
that was wrong. `sync_param_widgets` rebuilds a `ShownSnapshot` every frame from
the live model and rewrites any widget whose value drifted, and
`SelectionSet::apply_properties` replaces a node's whole properties record on
every `ObjectProperties` arrival, push included. It skips the focused field, so
an in-flight edit survives. And it never had the revert shape to begin with: its
writes are per-field commands (`SetObjectName`, `SetObjectDescription`,
`SetObjectPermissions` with a field + mask + set/clear, `SetObjectGroup`), never
a whole record. Its snapshot-diff-plus-focus-guard is in fact the design About
Land now copies.

**A prim's contents was broken, and quietly.** `ContentsEntry::serial` was
stored with the comment "so a *future* staleness check can compare it" — and
nothing did; `inventory_serial` had no reader anywhere in the viewer. It rides
**only** in the properties record, so a listing cached against a serial the
viewer never saw advance was stale with no way to notice.
`TaskInventoryCache::is_stale_against` now answers that, and a properties record
whose serial disagrees re-fetches through the same `reconcile_after_mutation`
path a local mutation uses — old items stay on screen until the reply lands.
Compared for **inequality, not order**: the serial is an `i16` and wraps.

**The Region floater had the bug in a sharper form.** It re-seeds its drafts
whenever the region data changes, so it looked immune — but `seed_draft` and
`seed_debug_draft` read the flags from `SlRegionIdentity`, which is written
**only** by the `RegionHandshake` that arrives on entry and never moves again.
`SlRegionLimits` is what every `RegionInfo` writes, and it carries the same
`RegionFlags` bitfield and the current maturity. So the form re-seeded from a
record frozen at arrival, and `SetRegionInfo` sends the whole form — every Apply
re-asserted entry-time flags and reverted whatever another estate manager had
changed. Both seeds now prefer the `RegionInfo`'s copy.

## Still open

[[viewer-region-push-discards-pending-edits]] — the Region floater's *other*
half. It re-seeds all three drafts wholesale on any region change, so a push
that arrives while an estate manager has ticked three boxes and not yet pressed
Apply throws those ticks away. That is the opposite defect from this entry's and
wants the same three-way merge on `RegionInfoUpdate` / `RegionDebugUpdate` /
`RegionTerrainUpdate`; it is filed separately rather than folded in here because
it is a different failure with a different symptom.

## Also done, same file

- `about_land.rs`'s private `parcel_update_from` is gone; it duplicated
  `ParcelInfo::to_update` field for field, differing only in spelling
  (`ParcelFlags::from_bits(parcel.raw_parcel_flags)` where `to_update` says
  `self.flags()` — the same function).
- `AboutLandDirty::access_values` is gone. It was set by `mark_all` and read by
  nothing; the Access tab is refreshed by `seed_edit_fields` and
  `update_editable_tab`.

## How it was verified

Unit tests, all client-side:

- `sl-proto`, on the merge itself: a push reaches an unedited field and not an
  edited one; a merge that changes nothing says so and is idempotent once the
  base advances; an edit that matches the grid stops being an edit.
- `sl-viewer-places`, on the floater's state: a record arriving before the draft
  is seeded changes nothing and does not adopt a base; a push reaches the fields
  this resident did not edit; merging advances the base.
- `sl-viewer-places`, on the region seed: the `RegionInfo`'s flags beat the
  handshake's, a push that clears every flag is still a push, and the handshake
  is used before any `RegionInfo`.
- `sl-viewer-edit`, on the contents serial: a moved serial makes a loaded
  listing stale, the fetch serial does not, a wrapped serial does, and an
  in-flight re-fetch is not stale again.

The grid half was already staged by [[test-fake-grid-concurrent-edits]]:
`an_about_land_save_reaches_the_parcels_other_occupant` in `client_end_to_end`
drives two sessions and asserts the whole argument — that a save built from the
record read at open reverts the other resident, and one built from the pushed
record does not. It passes before and after this change and is unaffected by it:
it is a protocol test that never instantiates the floater. It is what says the
merge is aimed at a real grid behaviour rather than an invented one.

Not verified live. Two residents on one parcel is the acceptance the entry
names and it needs two logins on a real grid; the fake-grid test covers the
protocol argument and the unit tests cover the floater's half of it.
