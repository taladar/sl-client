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

## Editing

An edit names an object by its region-local id and states one thing about
it: a `MultipleObjectUpdate` where it stands, how it is turned and how large
it is, an `ObjectName` its name, an `ObjectPermissions` the bits to set on or
clear from one of its masks, and so on, a message a field. What a grid
answers one with was read for the record's fields in
[Objects § Properties](objects.md#properties); this is the rest, and what a
grid makes of an edit it will not take as sent.

Measured on 2026-10-10 by the conformance case `object-edit`. One avatar
rezzes a handful of cubes on land it may build on, holds each selected as a
build floater does, and asks one question a leg, listening after each for
the object's updates and the form each comes in, its record, the kills, the
alerts, the inventory announcements and the instant messages; where an edit
is answered with nothing it asks for the object again
(`RequestMultipleObjects`), so that what it reads is the grid's and not what
it sent. It ran five times on aditi (Mauve, the public sandbox parcel) and
six on the local OpenSim. Every row below is checked on every run of both
live grids and of both fake flavours, but for the ones marked **not held**.

### What an edit is answered with

Each is the session's own edit of its own object. A transform was sent as a
build floater sends it, with the linkset bit set.

| edit | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a move (`MultipleObjectUpdate`, position) | a terse update | the same | the same |
| a turn (rotation) | a terse update | the same | the same |
| a resize (scale) | a full `ObjectUpdate` | the same | the same |
| a material, a flag (phantom), a shape | a full `ObjectUpdate` | the same | the same |
| a click action (`ObjectClickAction`) | a full `ObjectUpdate` | nothing: the value is stored, and the object reads it the next time something else sends it | each flavour as its grid |
| the search flag (`ObjectIncludeInSearch`) | a full `ObjectUpdate` | nothing, likewise | each flavour as its grid |
| a position, a rotation and a scale in one message (type 7, and 15 with the linkset bit) | taken; a full `ObjectUpdate` | dropped: the region logs an unknown `MultipleObjUpdate` packet type 7, and nothing is sent | each flavour as its grid |
| a category (`ObjectCategory`) | not taken: the record reads 0 | not taken: `Unhandled packet ObjectCategory` | not taken |

No build floater sends all three at once — the reference viewer sends a
position with a rotation, or a position with a scale — and neither does
ours. A client of its own making that does moves nothing on OpenSim and is
told nothing.

### Sizes, names and places a grid does not take

| what was sent | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a side of 1 mm | 0.01 m, in the full update that answers the resize; no alert | 0.001 m: taken | each flavour as its grid |
| a side of 1 km | 64 m, likewise | 256 m, likewise | each flavour as its grid |
| a name of 254 bytes | its first 63 | the same | the same |
| a description of 254 bytes | its first 127 | the same | the same |
| the name `a|b`, a line break, `c` | `a?b?c`: a question mark for the pipe and for the line break | `a|b`: cut at the line break | each flavour as its grid |
| the name `onéoff €` | `on??off ???`: a question mark for every byte that is not ASCII | `6F 6E C3 A9 6F 66 66 20 E2 82 AC 00`, twelve spaces and `on..off ....`: a hex dump of the field | each flavour as its grid |
| an empty name | an empty name | the same | the same |
| a move to 5,000 m up | a terse update, then the object is gone: killed, and filed in the owner's Lost And Found with the legacy `UpdateCreateInventoryItem` | taken: the object stands at 5,000 m | each flavour as its grid; the Second-Life flavour draws its line at 4,096 m (**the height not held**: only 5,000 m was sent) |
| a move to 50 m below zero, the ground being at 46 m | refused: a terse update of where the object stands, and no alert | taken | each flavour as its grid; the Second-Life flavour refuses a height below zero |
| a move 10 m off the region's edge, where no region is | a terse update, then gone to the Lost And Found, as above | refused: a full `ObjectUpdate` of where the object stands, and no alert | each flavour as its grid |
| what tells the owner the object went | an instant message of the dialog an object's has, from `Second Life` under **no** id, 1.1 to 2.3 s after the move: "Your object 'Object' has been returned to your inventory Lost and Found folder from parcel 'PARCEL' at Mauve 58, 62 because it went off-world." — the parcel's name is empty off the edge, and the spot is where the object was sent | — | the Second-Life flavour's, at once (**time not held**) |

The hex dump is OpenSim's `Util.FieldToString`, which is a debugging aid:
it takes a field with any byte outside printable ASCII for binary and
formats it sixteen bytes a line, terminator included, and `HandleObjectName`
and `HandleObjectDescription` store what it returns. A resident of an
OpenSim grid cannot give an object a name with an umlaut in it from a
viewer that sends UTF-8.

A return for going off-world is signed differently from a return the agent
asked for ([above](#what-a-derez-is-answered-with)): that one comes under
the agent's own id, this one under none.

Not measured: where exactly Second Life's ceiling and floor are; a move off
an edge another region lies beyond, which is a crossing; a size or a place
that is not a number.

### Masks

An `ObjectPermissions` names a mask, says whether to set or to clear, and
states the bits. The cube is its owner's, and its masks read
`0x7fffffff`, `0x7fffffff`, `0`, `0`, `0x00082000` (base, owner, group,
everyone, next owner) on Second Life and `0x0009e000` in the first two on
OpenSim before any of it. Each row follows the one above it.

| edit | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| next owner: set every bit | `0x7fffffff` | `0x0009e000`: what the base mask has | each flavour as its grid |
| next owner: clear every bit | `0x00002000`: transfer stays | `0x00082000`: transfer and move stay | each flavour as its grid |
| everyone: set copy and move | `0x00088000` | the same | the same |
| everyone: set modify and transfer | `0x0008a000`: transfer is set, modify is not | `0x0008e000`: both | each flavour as its grid |
| everyone: set every bit | `0x7fffbfff`: every bit but modify | `0x0008e000`: what the base mask has, without export | each flavour as its grid |
| everyone: clear every bit | `0` | `0` | `0` |
| group: set move, copy, modify and transfer | `0x0008e000` | the same | the same |
| group: clear every bit | `0` | `0` | `0` |
| owner: clear transfer | the owner mask reads `0x7fffdfff`: an owner may take a bit off its own mask | unchanged | each flavour as its grid |
| base: clear transfer | unchanged | unchanged | unchanged |

OpenSim's rules are its `SceneObjectPart.UpdatePermissions`: every mask is
kept within the base mask, export is set for everyone only where the owner,
base and next-owner masks all have it, a next-owner mask without copy is
given transfer, and move is always put back on it. Second Life's are read
off the table: a next-owner mask without copy keeps transfer and nothing
else is forced onto it, and modify is the one bit everyone cannot be given.

### A group and a deed

| request | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| an `ObjectGroup` naming a group the agent is not in | the object's group stays none; no alert | the same | the same |
| an `ObjectOwner` deeding the object to that group | the object stays the agent's; an `AlertMessage`: "Deed failed because you do not have permission to deed objects for your group.", with an `AlertInfo` keyed `DeedFailedNoPermToDeedForGroup` | the object stays the agent's; nothing said | each flavour as its grid |

The fake grid has no groups, so an agent of its is in none and every group
set and every deed is one of these two. It used to set whatever group it
was told and hand the object to it.

### Undo

An `Undo` names objects by their full ids and nothing else. Whatever comes
back is the simulator's own record of them.

The case renames a cube and puts it up for sale, then sets a material, a
click action, the phantom flag and a hollow, then resizes it, turns it and
moves it, and steps back nine times and forward nine times. Then, with
another cube, it makes two dozen moves of 10 cm and steps back until nothing
moves.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| what the first three steps back restore | the position, then the rotation, then the scale: the three transforms, last first | the same | the same |
| what the next six restore | nothing | nothing | nothing |
| what a step back never restores | the name, the sale, the material, the click action, the flag, the shape | the same | the same |
| what a step back is answered with | a terse update where it restores a position or a rotation, a full one where it restores a scale; no record | the same | the same |
| what a step back with nothing left is answered with | nothing | nothing | nothing |
| a `Redo` | forward through the same three, first first; the object ends as it was edited | the same | the same |
| how many moves back | 20 | 5 | each flavour as its grid |

The fake grid kept ten whole snapshots of an object, its record's fields
among them, so an undo there put a name back. It keeps placements now, as
many as its flavour's grid.

Not measured: whether an edit after an undo ends what a redo can reach,
which the fake grid assumes; how long a grid keeps a step (OpenSim's source
says five minutes).

### Duplicate

The cube that is copied has a name, a description, a next-owner mask with
copy set, a sale as a copy for L$25, a click action, a material and one
item in its contents.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| an `ObjectDuplicate` by an offset | a copy at that offset, in a full `ObjectUpdate`; no record unasked, no alert | the same | the same |
| the copy's name, description, masks, sale, click action, material, shape and size | the original's | the same | the same |
| its owner and creator | the agent | the same | the same |
| its creation date | the original's, to the microsecond | the same | the same |
| its last owner | the agent — also here, where the original, which nobody has taken, names none | the agent, as the original does | the agent |
| its item and source-task ids | none | none | none |
| its contents | the original's one item | the same | the same |
| its contents serial | the original's: 1 | 3, the original's being 1 | each flavour as its grid |
| an `ObjectDuplicateOnRay` aimed at nothing, the ray's end stated as the spot | a copy standing on that spot: its centre half its height above it | nothing: OpenSim copies only onto a prim | each flavour as its grid |
| one cast down at the original | a copy standing on top of it | the same | the same: the fake grid casts no ray and stands a copy on its target |
| a copy asked for with `FLAGS_CREATE_SELECTED` | arrives with that flag in its update | arrives without it | each flavour as its grid |

The fake grid used to copy an object without its contents, and did not
answer a duplicate on a ray.

Our `Command::DuplicateObjects` states duplicate flags of zero, which is
what the reference viewer sends for the copy a shift-drag leaves behind —
the one duplicate our viewer makes. Its Duplicate command asks for the copy
selected; that flag was measured on the ray form, which carries the flags
as a field.

## Somebody else's object

Measured on 2026-10-10 by the conformance case `object-edit-stranger`. The
**owner** rezzes and names a cube; the **stranger**, a second avatar that
holds no right over it — not its owner's friend with edit rights, not an
estate manager — sends everything a build floater and a pie menu can. After
each attempt the owner asks the grid for the cube and its record again, so
that whether the attempt took is read from the grid. It ran three times on
aditi and three on the local OpenSim (`--secondary resident`); every row is
checked on every run. The fake grid lets anybody edit anything
(`server-fake-grid-edit-permission-enforcement`), so these are the live
grids' answers alone.

Nothing a stranger sent changed the cube on either grid, with one
exception: **on OpenSim a stranger's `Undo` steps back out of the owner's
move.** Its handler asks nobody's permission.

### With no right over it

What the stranger's update flags say it may do with the cube: transfer and
"the owner may modify" on Second Life, transfer on OpenSim.

| attempt | Second Life | OpenSim |
| --- | --- | --- |
| a move | a terse update of the cube where it stands, and an `AlertMessage`: "Can't reposition -- permission denied" | a full `ObjectUpdate` of the cube where it stands; no alert |
| a turn | the same, the alert being "Can't rotate -- permission denied" | nothing |
| a resize | a full `ObjectUpdate` of the cube as it is, and "Can't rescale -- permission denied" | nothing |
| a shape | a full `ObjectUpdate`, and "You are not allowed to change this shape." | nothing |
| the phantom flag | a full `ObjectUpdate`, and "You don't have permissions to modify that object" | nothing |
| a material, a click action | nothing | nothing |
| a name, a description | the cube's record, unchanged | nothing |
| a sale | the cube's record, unchanged | an `AlertMessage`: "/You don't have permission to set object on sale" |
| a mask | a full `ObjectUpdate` and the record, both unchanged | the same |
| an `Undo` of a move the owner made | nothing, and the cube stays | **the cube goes back**, and a terse update says so |
| a duplicate | no copy; "Copy failed because you lack permission to copy the object 'NAME'.", keyed `NoCopyPermsNoObject` | no copy; nothing |
| a take of a copy | "You cannot copy your selection because you do not have permission to copy the object 'NAME'.", keyed `NoCopyNoSelCopy` | "No permission to take copy object NAME" |
| a take, a return | "Removal of the object 'NAME' from the simulator is disallowed by the permissions system.", keyed `NoPermsNoRemoval` | nothing |
| a delete to the Trash, an `ObjectDelete` | nothing | nothing |

Every update a refused edit is answered with shows the object as the grid
has it: it is how the grid puts a viewer right that has already drawn the
edit.

### With the move bit set for everyone

The owner sets move on the everyone mask. The stranger's flags gain move.

| attempt | Second Life | OpenSim |
| --- | --- | --- |
| a move, a turn | taken; a terse update | the same |
| a resize | refused as before, with the alert | refused as before, in silence |
| a name | refused: the record, unchanged | refused: nothing |
| a take | refused, `NoPermsNoRemoval` | refused: nothing |

### With the copy bit set for everyone

The owner sets copy on the next-owner mask and takes modify off it, and
sets copy on the everyone mask: the cube's masks read next owner
`0x0008a000`, everyone `0x00088000`. The stranger's flags gain copy.

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| the stranger's duplicate | a copy, the stranger's | the same |
| the copy's creator and last owner | the original's creator; the original's owner | the same |
| the copy's masks | base, owner and next owner `0x0008a000`: the original's next-owner mask; everyone `0x00088000`, as the original's | base `0x0009a000`, which keeps export; owner and next owner `0x0008a000`; everyone `0x00088000` |
| the stranger's take of a copy | an item in its Objects folder, by the legacy announcement; the cube stays, and its owner is told nothing | the same |
| that item's masks | base and owner `0x0008a000` | base and owner `0x0008a005`: OpenSim's folded bits |

### A member without the power

The stranger sets a cube of its own to a group it is a plain member of and
deeds it. On Second Life the group is one the avatar was in already; on
OpenSim the owner founds one for the run and the stranger joins it.

| request | Second Life | OpenSim |
| --- | --- | --- |
| the `ObjectGroup` | taken: the record names the group | the same |
| the `ObjectOwner` | refused with the alert `DeedFailedNoPermToDeedForGroup`, as a deed to a group the agent is not in | refused: the cube's record, with the agent still its owner |

Not measured: a deed by a member who has the power, which a plain resident
cannot take back; an owner's friend with edit rights; an estate manager;
what the owner is told of any of it beyond what it asks.

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

An edit, by the same measurements:

- A move, a turn and a resize are drawn the moment they are made and sent
  as the reference viewer sends them — a position, a position with a
  rotation, a position with a scale — so OpenSim's dropped combination
  never leaves our viewer. What the Build window and the world show
  afterwards is what the grid answers: a side of a kilometre reads 64 m on
  Second Life and 256 m on OpenSim, and a move the grid refuses is put back
  by the update it is refused with. The viewer itself keeps a typed size
  within OpenSim's limits, the wider of the two.
- A field of the Build window that was committed with `Enter` keeps the
  focus, and used to keep reading what was typed for it, whatever the grid
  kept. It follows the object again from the commit on, and is left alone
  only while it is being typed into; and a field nothing was typed into no
  longer sends its row again on losing the focus, which cost the grid a
  step of undo each time.
- Nothing waits for an answer to an edit, so OpenSim's silence over a click
  action or the search flag costs nothing.
- An object sent out of the world on Second Life leaves on its kill, its
  item appears in the Lost And Found, and the instant message is shown as
  the grid worded it.
- Undo and redo name the selection and leave the rest to the grid.
- A name is sent as it is typed. What an OpenSim region makes of one that
  is not ASCII is shown as the region kept it, hex dump and all, the next
  time the object is selected.

Checked through the viewer automation (`e2e_objects`): on each fake
flavour, a prim rezzed and named through the Build window is given a side
of a kilometre and reads its grid's largest in the window and in the
world; a move under the ground (Second Life) or off the region's edge
(OpenSim) is put back in both by the grid's update; and on the Second-Life
flavour a move off the edge takes the prim out of the world and shows its
item in the inventory window. The same test runs against a live grid with
`SL_E2E_GRID=opensim` and `=aditi`.

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
