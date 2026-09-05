---
id: viewer-floaters-never-reread-after-a-push
title: A floater seeds its draft once and reverts what it never saw
topic: viewer
status: bugs
origin: doing test-fake-grid-concurrent-edits (2026-09-05)
points: 3
refs:
  [
    test-fake-grid-concurrent-edits,
    test-asset-save-mutation-survey,
    viewer-task-inventory-open-and-save-back,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

[[test-fake-grid-concurrent-edits]] gave the fake grid the pushes a simulator
sends when somebody *else* changes what you are looking at: an object's
properties to whoever holds it selected, a sequence-zero `ParcelProperties` to
the parcel's occupants, a `RegionInfo` to the region. Nothing in the viewer
consumes any of them as a re-read.

`sl-viewer-places/src/about_land.rs` is the sharp case, and the one with a
test staging the exact failure
(`an_about_land_save_reaches_the_parcels_other_occupant`).
`AboutLandState::seed_draft` is gated on `draft_ready`, so the edit draft is
built once per open and never again. `ParcelPropertiesUpdate` carries the
**whole** record, so **Apply** then asserts every field as it stood when the
floater opened — silently reverting whatever another resident changed in
between. No grid stops this: a simulator cannot tell a re-asserted field from
an unchanged one, and Second Life has no edit lock to have prevented the
overlap in the first place.

The fix is not to build the draft more carefully but to re-seed it: on an
unsolicited `ParcelProperties` for the bound parcel, carry the pushed values
into every field the resident has not edited since the floater opened. What
"has not edited" means is the design question — the cheap answer is to re-seed
the whole draft and lose an in-flight edit, the honest one is per-field dirty
tracking, which the floater's other in-place refresh passes already have the
shape for (`AboutLandDirty`).

The same shape, unchecked, elsewhere:

- **The build floater** — an `ObjectProperties` push arrives for every
  selected object; a name/description/permissions panel populated at select
  and never re-read has the same revert.
- **A prim's contents** — `inventory_serial` is the whole freshness marker for
  a task inventory and it travels *only* in the properties record. A listing
  cached against a serial the viewer never saw advance is stale with no way to
  notice.
- **The Region/Estate floater** — `RegionInfo` reaches every avatar in the
  region, and the estate forms re-assert whole records the same way About Land
  does.

Also here, since it is the same file: `about_land.rs` carries a private
`parcel_update_from` that duplicates `ParcelInfo::to_update` field for field.
Two copies of "read the parcel into the form" is exactly one copy too many for
a conversion whose whole hazard is forgetting a field.

Acceptance: with two viewers on one parcel, the second resident's save after a
push carries the first's change forward rather than reverting it; the
duplicate conversion is gone; and the build floater and the region floater are
either fixed the same way or recorded here as still open with a reason.
