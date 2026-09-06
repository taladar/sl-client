---
id: viewer-region-push-discards-pending-edits
title: A region push throws away the estate manager's unapplied edits
topic: viewer
status: bugs
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

## Fix

The same three-way merge About Land now uses:
`ParcelUpdate::merge_unedited(base, fresh)` in `sl-proto`, with the base stored
beside the draft. Three drafts means three merges — `RegionInfoUpdate` (9
fields), `RegionDebugUpdate` (3), `RegionTerrainUpdate` (10) — and the same
question about the text fields, which here are the agent limit, object bonus,
water height, terrain raise / lower limits and sun hour. `AboutLandState`'s
`shown_fields` is the shape for that half: compare a widget against what it was
last *given*, not against the merge's base, which has already advanced.

Worth doing the merges as one generic helper rather than three copies — unlike
`ParcelUpdate` these three are small and structurally identical, so a derive or
a macro over "carry the fields that still equal base" would beat writing the
walk three times. `merge_unedited`'s destructure-without-`..` trick is what
makes a forgotten field a compile error and should survive whatever shape it
takes.

## How to verify

Client-side, the way About Land's half was: seed a draft, move one field, merge
a record that changes a *different* field, and assert both survive. The
two-manager case on a live grid is the acceptance but needs two estate-manager
logins; the fake grid already pushes `RegionConfigured` to every avatar in the
region, so the protocol half is staged.

Reference (Firestorm, read-only): `llfloaterregioninfo` — worth checking whether
it re-reads into a form with unsaved changes at all, or whether it simply leaves
the panel alone until Apply or Cancel.
