---
id: test-fake-grid-edit-surfaces
title: A viewer can edit nothing on the fake grid
topic: test
status: done
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

## Done (2026-09-05)

All three families, not one: the split above was a plan for picking it up,
and picking it up showed the three share more than a pattern — the object
family needed a full `ObjectProperties` sender before anything it wrote was
readable, and once that existed the parcel and estate halves were each an
afternoon rather than a task.

**`sl-proto`.** Twenty client messages left the `RAW_FORWARDED` ledger. The
object family became one typed `ServerEvent` per message and the *whole*
parcel family did too, so `PARCEL_FAMILY` is gone from
`tests/sim_session_symmetry.rs` and `OBJECT_FAMILY` is down to the three
grabs — which are not edits: a grab moves an object without changing what
the region holds of it. Four senders were missing and are now there:
`send_object_properties` (the full form, which nothing had; the condensed
family reply existed), `send_parcel_access_list_reply`, `send_region_info`,
and `send_estate_owner_message` with `send_estate_info` /
`send_estate_access_list` over it. Supporting decoders and conversions:
`object_transform_from_wire` (with the quaternion *unpack* that had never
been needed before — the packer threw the real component away and nothing
had ever put one back), `PermissionField::{from_code, select_mut}`,
`EstateAccessDelta::{from_u32, list, is_add}`, `ParcelInfo::to_update` (what
a viewer's About Land floater does: populate the form from the record it
last read) and a `prim_flags` module for the four `PrimFlags` bits this
workspace sets or reads.

**`sl-fake-grid`.** Three modules, each answering under the region's own
lock: `object_edits.rs`, `parcel_edits.rs`, `estate.rs`. What was learned
building them, in the order it cost time:

- **Two stores, two messages.** An object's name, description, category,
  sale state, permissions and owner do not appear in an `ObjectUpdate` at
  all. Every one of those edits is invisible until the properties message
  exists, which is why the read-back had to be built before the writes could
  be tested — and why `ObjectProperties` is synthesised from the object on
  first ask (`SceneFixtures::properties_of`) rather than stated by fixtures:
  a prim nobody has named is a prim named `Object`, not a prim with no
  record.
- **The contents serial lives with the inventory, not with the record.**
  `properties_of` reads it from `task_inventories` every time, because a
  record carrying its own copy would eventually tell a viewer its cached
  listing was still good.
- **Linking is not a parent id.** A child's placement is stated in its
  root's frame, so a link has to restate it and a delink has to put it back
  — including the rotation, which needs the quaternion product a naive
  subtraction skips. `into_frame` / `out_of_frame`, with the round trip
  pinned by a unit test through a turned root.
- **Undo is a restore, not an inverse.** The `Undo` / `Redo` messages name
  objects and nothing else — and by *full* id, the one place in the object
  family that is true — so what one step undoes is whatever the region
  recorded. Each edited object carries a short stack of whole `Object`
  snapshots; a fresh edit abandons the redo branch. A grid with nothing
  recorded answers with silence, which is what OpenSim does and what the
  conformance case allows for.
- **A parcel has one record and the form carries all of it.** Which is why
  `parcel-edit` asserts the *unchanged* fields came back unchanged: that is
  the failure a re-asserting form actually produces.
- **An estate command is a switch on a string.** One message, a method name
  and a list of byte parameters — where `setaccess` puts raw 16-byte ids in
  the same field `estateupdateinfo` fills with decimal text, so the sender
  takes bytes. The region's configuration and terrain composition became
  lazily-written stores: derived from the region's identity until an estate
  manager changes them, so a region nobody reconfigured cannot drift from
  its own handshake.
- **Every estate command is refused in silence** without the power, because
  that is what OpenSim does and it is the only thing that makes the gate
  observable. The single `refreshmapvisibility` arm moved out of
  `agent_requests.rs`, which is now about the agent again.

Two fixes fell out of it. The fake grid's regions reported `sim_owner` as
the **nil** key and `is_estate_manager` as false for everyone — a grid whose
About Land and Region/Estate floaters both named "(nobody)" and whose estate
replies disagreed with the account issuing the commands. A region is now
owned by the grid's first estate-manager account, and the flag is per
session. And the conformance harness's three fake accounts have **fixed**
agent ids, so the harness can hand a case "the other avatar"
(`Fixtures::with_other_avatar`) without logging that avatar in.

**Offline conformance: 22 cases became 30.** Seven cases stopped waiting for
a grid somebody has to stand up — `object-edit` (extended to cover the
transform, the click action, the category, the search flag and the undo
stack), `object-link-delink`, `object-properties`, `region-info`,
`estate-info`, `estate-access` — plus one new case, `parcel-edit`, for the
About Land form and the ban list.

Two things deliberately left:

- **`parcel-access-list` stays live-only.** It asserts the querying avatar
  *owns* the parcel, and the catalogue region's land is owned by a fixture
  constant rather than by an account. Making that true offline means the
  scenario's parcels and prims taking the grid's estate owner at start-up,
  which is a change to every fixture rather than to this surface;
  `parcel-edit` covers the same messages offline in the meantime.
- **`RezObjectFromInventory`** — rezzing an object *item* back into the
  world — is still unanswered, and stays that way until
  [[test-assets-object-asset-codec]] gives an object item bytes to rez from.
  A grid that rezzed a default cube for any object item would be answering a
  question it cannot answer, and `object-rez-derez` would pass on a lie.

Followed by [[test-fake-grid-concurrent-edits]], which is the push half:
these edits reach the region and the client that made them, and telling the
region's *other* viewers about a properties change needs the selection
subscription that task owns.
