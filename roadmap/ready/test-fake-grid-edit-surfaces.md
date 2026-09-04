---
id: test-fake-grid-edit-surfaces
title: A viewer can edit nothing on the fake grid
topic: test
status: ready
origin: scoping test-fake-grid-asset-round-trip (2026-09-05)
points: 8
refs:
  [
    test-fake-grid-object-write-path,
    test-fake-grid-simulator-request-surfaces,
    test-fake-grid-concurrent-edits,
    test-object-properties,
  ]
---

Context: [context/testing.md](../context/testing.md).

[[test-fake-grid-object-write-path]] gave the fake grid three writes — rez,
derez, and a drop into a prim's task inventory. They are still the *only*
three. Everything else a viewer can change is decoded by `SimSession` and
dropped on the floor, which means no tier below a live grid can answer "did
my edit reach the grid", and the region a test looks at is always the region
its fixture stated.

The `RAW_FORWARDED` ledger is the list, and it is most of the build floater:

- **An object** — `ObjectName`, `ObjectDescription`, `ObjectCategory`,
  `ObjectClickAction`, `ObjectMaterial`, `ObjectSaleInfo`, `ObjectFlagUpdate`,
  `ObjectIncludeInSearch`, `ObjectPermissions`, `ObjectGroup`, `ObjectOwner`,
  `ObjectLink`, `ObjectDelink`, `ObjectDuplicate`, `ObjectDelete`,
  `MultipleObjectUpdate` (the transform), `Undo` / `Redo`. Not
  `ObjectExtraParams`, which already has `ServerEvent::ObjectExtraParamsSet`
  and shows the pattern.
- **A parcel** — `ParcelPropertiesUpdate` (the whole About Land form),
  `ParcelAccessListUpdate`, `ParcelBuy`, `ParcelDeedToGroup`,
  `ParcelRelease`, `ParcelReclaim`, `ParcelReturnObjects`.
- **A region and its estate** — `RequestRegionInfo` is raw; the estate half
  *does* arrive typed as `ServerEvent::EstateOwnerRequest`, and
  `agent_requests.rs` answers exactly one method of it
  (`REFRESH_MAP_VISIBILITY`) while the rest — terrain textures and heights,
  region flags, access lists, the covenant — go nowhere.

This is the same shape as [[test-fake-grid-simulator-request-surfaces]], one
family along: a message `SimSession` decodes with no `ServerEvent` and no
`send_*` counterpart, so no simulator built on it can answer. The difference
is that these are *writes*, so each also needs somewhere in the region to
land — which [[test-fake-grid-object-write-path]] has now built for objects
(the region-scoped world) and which parcels already have
(`SceneFixtures::parcels`, today read-only).

Worth splitting when someone picks it up: the object family, the parcel
family and the region/estate family are three independent bodies of work
sharing one pattern, and 8 points is a guess at the whole rather than a plan
for it. The object family is the one with an existing store and an existing
test to extend.

Not in scope: telling *other* viewers. That is
[[test-fake-grid-concurrent-edits]], and it is deliberately separate — an
edit that sticks is useful on its own (a viewer can finally be tested for
"my change took effect"), and the push half needs a subscription model this
task does not.

Acceptance: a client edit of an object's name, an object's transform and a
parcel's properties each change what the region holds, and a refetch by the
same client returns the changed record rather than the fixture's.
