# Building

## Rez and take

An object leaves a region by a `DeRezObject`, which names where it is to go:
into the agent's inventory with the object left standing (a copy), into the
inventory (a take), into the Trash (a delete), or back to its owner (a
return). It comes back by a `RezObject` naming the inventory item. An
`ObjectDelete` is the reference viewer's force-delete, and an object flagged
temporary leaves by itself.

Measured on 2026-10-10 by the conformance case `object-rez-derez`, which
rezzes a cube, names it, and asks one question a leg, listening after each
for the kills, the inventory announcements, a `DeRezAck`, the alerts and the
instant messages: a copy, a take, a restore of the taken item to where it
stood, a rez of it and a select of what it rezzed, an `ObjectDelete`, a
delete naming the Trash and one naming the Objects folder, a return of the
agent's own object, a take and a rez of a linkset of two, and three prims
flagged temporary eight seconds apart. It ran four times on aditi (Mauve,
the public sandbox parcel) and four on the local OpenSim, and a fifth on
each with the restore, which the first four had not got. Every row below
but the ones marked **not held** is checked on every run of both live grids
and of both fake flavours; **live only** rows on the live grids.

### What a derez is answered with

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the item it files | announced with the legacy UDP `UpdateCreateInventoryItem`, for a copy, a take, a delete and a return alike; no `BulkUpdateInventory` | the same | the same |
| the transaction that announcement states | an id that is not the derez's | nil | each flavour as its grid |
| a `DeRezAck` | none | none | none |
| the kill, of a lone prim | one `KillObject` naming it, 0.35 s after the request and 40 ms after the announcement | two, each naming it, in the same instant as the announcement | each flavour as its grid, with the announcement (**times not held**) |
| the kill, of a linkset of two | one `KillObject` naming the root and the child | two, each naming the root alone | each flavour as its grid |
| a copy | the object stays; nothing but the item | the same | the same |
| a linkset | one item | the same | the same |
| a delete naming the Trash | the item in the Trash | the same | the same |
| a delete naming the Objects folder | the item in the Objects folder | the item in the Trash: OpenSim finds the agent's own, whatever the derez named | each flavour as its grid |
| a return of the agent's own object | the item in its Lost And Found | the same | the same |
| what tells the agent of a return | an instant message of the dialog an object's has, from `Second Life` under the agent's own id, a second after the kill: "Your object '…' has been returned to your inventory Lost and Found folder by you from parcel '…' at Mauve 56, 62." | one from `Server` under no id, with the region's next backup — up to twenty seconds later, and one for all of an agent's objects returned since the last: "Your object … was returned from <124.5, 128, 27.5> in region Default Region due to parcel owner return" | each flavour's message, at once |

The fake grid's Second-Life flavour used to announce a take with a
`BulkUpdateInventory` over the event queue, as did everything of ours that
described the two grids: "inventory moved behind AIS3, so the take's
announcement went with it". Nothing had measured it. A client still has to
take both forms — the bulk one is how Second Life announces other changes to
an inventory — and `FakeGridBuilder::inventory_announcement` builds a grid
that announces a take with it.

### The item a take makes

| field | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| name and description | the object's | the same | the same |
| asset id | nil: a viewer is not told where the object's body is | an asset's, which it may fetch | each flavour as its grid |
| asset and inventory type | 6 and 6: an object | the same | the same |
| flags, sale type, group | 0, not for sale, none | the same | the same |
| owner and creator | the agent | the same | the same |
| creation date | when it was derezzed (**live only**) | the same | a fixed date, so that a seeded grid mints the same run twice |
| base and owner masks | the object's: `0x7fffffff` for a prim nobody edited | `0x0008e00f`: the object's `0x0009e000` without export, with the four bits OpenSim folds a linkset's permissions into | each flavour as its grid |
| group, everyone and next-owner masks | the object's: `0`, `0`, `0x00082000` | the same | the same |

A copy's item is the same as a take's.

### A rez

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| what arrives | the object in a full `ObjectUpdate`; no record unasked, no inventory announcement | the same | the same |
| the item | stays where it was, being one its owner may copy | the same | the same |
| a linkset's item | both prims back | the same | the same |
| the record's creation date | the date of the object the item was made from, to the microsecond | the same | the same |
| its name, description and masks | the item's, which are that object's | the same | the same |
| its item id | the item's | the same | the same |
| its folder id | the item's folder | none | each flavour as its grid |
| its last owner | the agent — also on Second Life, where the object the item was made from had none | the agent | the agent |
| its source task, its contents serial | none, 0 | the same | the same |
| the checksum and masks the `RezObject` states | not checked: a rez with a checksum of zero works | the same | the same |
| a `RezRestoreToWorld` of the item (Restore to Last Position) | the object back where the one it was made from stood; the item stays; nothing said | nothing: OpenSim reads the message and nothing listens for it | each flavour as its grid |

Our own Linden-text bridge (`PrimBlock::to_properties`, `sl-object-asset`)
left a rezzed prim's creation date at zero, taking the asset's `birthtime`
for something else than the record's date. They are the same stamp, in
microseconds, and it now carries it both ways.

### `ObjectDelete`

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| an `ObjectDelete` of the agent's own object | the object is killed and no item is made of it: not in the Trash, not anywhere | nothing: OpenSim has no handler for the message | each flavour as its grid |

### Temporary prims

Three cubes were rezzed four seconds apart and each flagged temporary
(`ObjectFlagUpdate`) as it appeared.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| how long each stood (**live only**) | 60.3 to 66.1 s after it was flagged, each in its own time | 60.8 to 75.0 s: a prim expires a minute after it is flagged and is taken away by a sweep every 180 frames, so prims flagged eight seconds apart went in one | they stand: the fake grid keeps no clock for a prim |
| what the owner is sent | the kill; no item, no alert | the same | — |

A first reading of this leg had Second Life's prims gone after ten and
eighteen seconds. It had timed the first kill of *anything* in the window,
and a public sandbox is full of other people's temporary prims.

## Land that says no

Measured on 2026-10-10 by the conformance case `object-rez-land`. Its
**builder** makes an object item where it may build and then, on land that
does not let it, sends an `ObjectAdd` and a `RezObject` of that item. On the
local OpenSim the land's owner — a second avatar — takes "everyone may
build" off the parcel the builder stands on and puts it back, then returns
a cube of the builder's with `DRD_RETURN_TO_OWNER`, then sets the parcel to
return other people's objects after a minute. On aditi we hold no land
(`gridspec-aditi-test-land`): the builder looks through its login region
for a parcel that is not open to building and does not keep avatars out,
flies there, tries, and flies back. It ran three times on each grid. The
refusals are checked on every run of both live grids and both fake
flavours, the owner's legs on every run of OpenSim.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| an `ObjectAdd` on land that does not let the agent build | nothing appears; an `AlertMessage`: "You cannot create objects here.  The owner of this land does not allow it.  Use the land tool to see land ownership.", with an `AlertInfo` keyed `CantCreateObjectParcelPerms` | nothing appears; an `AlertMessage`: "You cannot create objects here.", with no key | each flavour as its grid |
| a `RezObject` of an inventory item there | nothing appears; an `AlertMessage` with no key: "Can't rez object 'NAME' at { 109.188, 109.59, 45.4379 } on parcel 'PARCEL' in region Mauve because the owner of this land does not allow it.  Use the land tool to see land ownership." | nothing appears, and nothing is said | each flavour as its grid; the Second-Life flavour's alert states the spot to more digits (**not held** letter for letter) |
| the item of a refused rez | stays | stays | stays |
| the owner returns somebody's object | not measured: no land | the owner is sent the kill and nothing else — no item, no `DeRezAck`; the object's owner is sent the kill, an item in its Lost And Found by the legacy announcement, and with the next backup an instant message from `Server`: "… due to parcel owner return" | not imitated |
| a parcel that returns other people's objects after a minute | not measured: no land | 62 to 73 s after the rez, the same three things, the message ending "due to parcel autoreturn" | not imitated |

The reference viewer has no notification named
`CantCreateObjectParcelPerms`, and shows the alert's own text; so do we.

## What our viewer does with it

- A take, a copy, a delete and a return are each one `DeRezObject` from the
  object's pie; nothing waits for an answer. The object leaves the world on
  the kill — every prim under a root that is named alone, as OpenSim names
  it — and the item appears because the session keeps what either
  announcement carries.
- A rez out of the inventory is a drag into the world and waits for nothing
  either: on land that says no, Second Life's alert is shown as it comes,
  and on OpenSim nothing happens and nothing says why.
- A grid's alert and a return's instant message are shown as the grid worded
  them.

Checked through the viewer automation (`e2e_objects`): on each fake flavour
and on each live grid (`SL_E2E_GRID=opensim` and `=aditi`), a prim rezzed
and named through the Build window is taken from its pie and leaves the
world, its item shows in the inventory window under its name, a drag of
that item into the world puts an object of that name back, and Delete from
the pie takes it away again. On the live OpenSim the test fails now and
then, in the harness's first placement or in opening the last pie
(`test-e2e-objects-live-opensim-intermittent`).

The fake grid neither returns somebody else's object, nor returns anything
by itself, nor ends a temporary prim: `server-fake-grid-object-return`. It
tells an agent of a return at once on both flavours, where OpenSim waits for
its next backup — 3.6 and 5.0 s in two runs, and past a four-second window
in three others — and puts several returns into one message.

Not measured: a rez of an item its owner may not copy, which should take the
item with it (a prim's owner cannot make itself one); who else in the region
is sent the kill, and in which form; a `DRD_SAVE_INTO_AGENT_INVENTORY` and
the save into a prim's contents, which is `gridspec-task-inventory`'s; a
return by somebody else and an auto-return on Second Life; a rez onto a full
parcel or into a full region.
