---
id: viewer-region-push-discards-pending-edits
title: A region push throws away the estate manager's unapplied edits
topic: viewer
status: done
origin: doing [[viewer-floaters-never-reread-after-a-push]] (2026-09-06)
points: 3
refs: [viewer-floaters-never-reread-after-a-push, viewer-region-options-estate,
  viewer-region-options-terrain]
---

Context: [context/viewer.md](../context/viewer.md).

The Region/Estate floater's other half, split out from
[[viewer-floaters-never-reread-after-a-push]] because it is the *opposite*
failure and needs a different fix.

`refresh_on_region` (`sl-viewer-places/src/about_region.rs`) re-seeds all three
drafts — `RegionInfoUpdate`, `RegionDebugUpdate`, `RegionTerrainUpdate` —
whenever `SlRegionIdentity` or `SlRegionLimits` changes:

```text
let changed = identity.is_changed() || limits.as_ref().is_some_and(Ref::is_changed);
if !changed && state.draft_seeded { return; }
state.draft = seed_draft(&identity, limits.as_deref());
state.debug_draft = seed_debug_draft(&identity, limits.as_deref());
state.terrain_draft = seed_terrain_draft(&identity, limits.as_deref());
```

That is what keeps the form from going stale, and it is why this floater never
had the never-re-reads bug. But it is unconditional: an estate manager who has
ticked three boxes, typed an agent limit and not yet pressed **Apply** loses all
of it the moment a `RegionInfo` lands. And one lands unasked whenever *another*
estate manager saves — [[test-fake-grid-concurrent-edits]] made the fake grid
send exactly that, to everyone in the region.

Worse than it sounds, because the components are **inserted** rather than
mutated (`sl-client-bevy/src/world.rs`), and in Bevy an insert marks a component
changed whether or not anything in it moved. So a `RegionInfo` that reports
nothing new still wipes the form.

## Fixed (2026-09-13)

The three-way merge About Land uses, generalised. `merge_unedited` moved out of
`ParcelUpdate`'s `impl` into `sl-proto/src/types/merge.rs` as a macro, and now
generates the same walk for four record types instead of one:
`ParcelUpdate` (18 fields), `RegionInfoUpdate` (9), `RegionDebugUpdate` (3) and
`RegionTerrainUpdate` (9 — the task said ten; three of them are arrays of four,
and an array merges whole, so retyping one corner band makes all four that
manager's). The destructure-without-`..` survived the move
intact — it is the macro's field list, so a field added to any of the four types
and not to its list is still a compile error, and a listed name that is not a
field is one too.

In the floater, `AboutRegionState::draft_seeded: bool` became
`seeded: Option<RegionBases>` — the three records the drafts were last seeded or
merged from, read together because they answer the same `RegionHandshake` plus
`RegionInfo` pair. `merge_region` seeds outright when there is no base (the
first record a window reads, and the one a **re-open** asks for) and merges
otherwise.

The text fields got About Land's other half, `shown_fields`: they are not
mirrored into a draft until Apply reads them, so the merge cannot see typing in
flight and the widget is compared against what it was last *given* instead.
Thirteen of them here — agent limit, object bonus, water height, terrain raise /
lower limits and the eight per-corner band fields. **Not** the sun hour: it is
in `RegionTerrainUpdate` and merged with the rest, but this floater has no
widget for it (the Environment tab owns the sun), so the task's list was one
too long.
The maturity combo and the four texture swatches need no such guard — a pick
writes straight into the draft, where the merge sees and keeps it.

`FieldSeed`, `seed_one_field`, `set_field_text` and `set_combo` moved to a new
crate-private `sl-viewer-places/src/edit_fields.rs`, since both floaters now
want all four.

### The second push, and a bug in the shape this was copied from

Writing the headless test for two pushes in a row found that About Land's half
survived **one** push and not the next, and the region floater inherited it with
the pattern. On a declined write, `shown_fields` recorded what the widget
currently *read* — the resident's own text. The next push then compared that
typing against itself, matched, concluded nobody had typed there, and overwrote
it. Since a busy estate pushes a `RegionInfo` on every manager's save, surviving
exactly one was barely a fix at all.

`shown_fields` is what a field was last **given**, and a declined write gives it
nothing, so `seed_one_field` now keeps the previous value there instead. Both
floaters take the correction, since the helper is now shared, and
`typing_survives_a_region_push` pins it by pushing twice.

### Better than the reference here, deliberately

`llfloaterregioninfo.cpp`'s `processRegionInfo` answers the question the task
asked: it walks all three panels calling `setValue` on every control
unconditionally, so Firestorm **does** clobber an unsaved form on any
`RegionInfo` — including the one another manager's save pushes. Keeping the
pending edits is a departure from the reference, and the right one: the same
argument as About Land's, that a form which re-asserts what it read at open
silently reverts whoever saved in between.

## How it was verified

Client-side, the way About Land's half was.

- `sl-proto/src/types/map.rs` — four tests over the generated merges: a push
  reaching the untouched fields and leaving the edited ones, the base advance
  that makes a repeated record one change, the all-bool debug record, and the
  terrain arrays (which move as whole arrays, so one retyped corner protects
  its four).
- `sl-viewer-places/src/about_region.rs` —
  `the_first_record_seeds_rather_than_merges`,
  `a_push_leaves_the_edits_this_manager_has_not_applied`,
  `an_unchanged_record_moves_no_draft`, and two headless app tests:
  `typing_survives_a_region_push` (a retyped water height survives two pushes
  while an untouched band field takes both) and
  `reopening_discards_the_typing_it_re_asks_for`.

The two-manager case on a live grid remains the acceptance and still needs two
estate-manager logins; the fake grid already pushes `RegionConfigured` to every
avatar in the region, so the protocol half is staged.
