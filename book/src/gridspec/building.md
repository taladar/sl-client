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

## Linking

An `ObjectLink` names prims by their region-local ids, the first of them
the root the rest are to hang off; an `ObjectDelink` names the prims to
take out of their sets. Neither is answered with a message of its own. And
nothing a simulator sends states a **link number** — the root's 1, its
children's 2 and up — though half the script library addresses a prim by
one: a viewer can only watch which prim hangs off which, and a script is
the one thing that can say where each stands.

Measured on 2026-10-11 by the conformance case `object-link-delink`. One
avatar rezzes four cubes on land it may build on, names them `A` to `D`,
and puts a script into each that says, at its start and whenever its links
change, its own link number and the names of the set's prims in the order
of theirs (`llGetLinkNumber`, `llGetLinkName`, over `llOwnerSay`). It then
asks one question a leg, listening after each for the updates and the form
each comes in, the records, the alerts and what the scripts say. It ran
four times on aditi (Mauve, the public sandbox parcel) and four on the
local OpenSim, and once more on aditi with three cubes in a line linked in
all six orders at four spacings. Every row below is checked on every run of
both live grids and of both fake flavours, but for the ones marked **not
held** and the ones marked **live only**, which are what the scripts said:
the fake grid's scripts know nothing of link numbers yet
(`server-world-link-sets`).

### What a link and a delink are answered with

| request | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a link of four free prims in one message, each held selected | every prim of the new set again, each in a full `ObjectUpdate` — the last link number first and the root last — and the **root's** record; no alert | every prim again in a full `ObjectUpdate`, the root first and then the link numbers in order, and the root's record; no alert | each flavour as its grid |
| a link of prims nobody holds selected | takes, and no record comes | takes, and the root's record comes all the same | each flavour as its grid |
| where a linked child reads as standing | at its offset from the root, in the root's frame: a root turned a quarter turn about z has a child that stood along the region's x at its own −y | the same | the same |
| a link that names a child of another set, which no viewer sends | the child's whole set joins | the same | the same |
| a link of one prim | nothing | nothing | nothing |
| a delink of one child | the child stands free where it stood and the rest stays linked; the child, the root and a sibling each come again in a full `ObjectUpdate`; no record, no alert | the same | the same |
| a delink of the root alone | the root stands free where it stood, and **another prim becomes the root of the rest**, where it stood; the old root, the new one and a child each come again in a full `ObjectUpdate`; no record, no alert | the same — but for which prim it is, below | each flavour as its grid |
| a delink that names every prim of a set, as a viewer's Unlink does | every prim free; each again in a full `ObjectUpdate` | the same | the same |
| an `ObjectDuplicate` of a linkset's root | a copy of the whole set | the same | the same |

An earlier reading had Second Life sending a link's *children's* records
([Objects § Properties](objects.md#properties), 2026-10-09). That was the
answer to the rename the case sent just before its link, heard in the
link's window; the record a link sends is the root's, and on Second Life
only to a linker that holds the root selected.

### The order of a linkset

What the scripts said of each set, and which prim became the root when the
root was delinked alone — again and again, until nothing was left linked.

| set | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a prim on its own (**live only**) | says its link number is 0 | the same | — |
| `A B C D`, linked in one message (**live only**) | numbered `A B C D`: the order they were named in | the same | — |
| `C` linked into the set `A B` (**live only**) | numbered `A C B`: the newcomer is 2 | numbered `A B C`: the newcomer is last | — |
| the set `C D` linked into the set `A B` (**live only**) | numbered `A C D B`: the set that joined comes whole, in its own order, ahead of the prims the root had | numbered `A B C D`: it comes after them | — |
| which prim a root delinked alone is replaced by | **link number 2**, the rest keeping their order: `A B C D` without `A` is numbered `B C D` | **not a link number**: of the three sets above it was number 2, number 3 and number 4, and the rest was renumbered from it — `A B C D` without `A` was numbered `D C B` | each flavour as its grid |
| the order the roots came out in, delinking the root until none was left | `A B C D`, `A C B`, `A C D B`: the link numbers | `A B C D`, `A C B`, `A D C B` | each flavour as its grid |

So "the prim linked last is 2" holds on Second Life and not on OpenSim,
where a prim keeps the number it was given and a newcomer takes the next.

OpenSim's choice of a new root is an accident with a rule. `DelinkObjects`
takes the first of `SceneObjectGroup.Parts`, and a group keeps its parts
in a dictionary, which lists them slot by slot and hands a new part the
slot the part removed last gave up. A group nothing was ever removed from
lists its parts in the order they joined, which is the order of their link
numbers; one that has lost parts and gained others does not. Followed
through every link and delink of the case, that rule gives each of the
orders above, and the fake grid's OpenSim flavour keeps the same slots
(`link_sets::Parts`). The prim that is promoted then has the rest linked
under it in that order, so the link numbers follow the dictionary from
there on.

### Links a grid refuses

The distance leg moves one cube straight up from another and links the two,
halving the distance until the furthest that links is known to the metre.
The counting leg copies a linkset and joins it to its copy, eight times
over, in a lattice a few metres across.

| link | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| two cubes half a metre across | link 64 m apart, centre to centre, and not 65 m | link 256 m apart, the furthest tried | each flavour as its grid |
| two cubes ten metres across | the same: 64 m and not 65 m | the same: 256 m | each flavour as its grid |
| what a link too far is answered with | nothing links, and an `AlertMessage`: "Link failed -- Unable to link any pieces - pieces are too far apart.", keyed `LinkFailed`, stating `reason` 1, `total_pieces` 2 and `linkable_pieces` 1 | — | each flavour as its grid |
| three cubes in a line 28 m apart, the ends 56 m | all three link, from an end and from the middle | — | each flavour as its grid |
| three in a line 48 m apart, the ends 96 m | **nothing links, whichever is the root** — the middle one included, which stands 48 m from both; the same alert, with `total_pieces` 3 and `linkable_pieces` 2 | — | each flavour as its grid |
| one more prim to a linkset of 256 | nothing links: "Link failed -- Unable to link any pieces - too many pieces.", keyed `LinkFailed`, `reason` 2 — with the set as the root and with the prim as the root | links: a set of 257 | each flavour as its grid |
| a prim its owner has taken modify off (`ObjectPermissions`, the owner mask) | nothing links, as a child and as a root: "Link failed because you do not have edit permission.", keyed `LinkFailedNoPermToEdit` | links: an owner cannot take a bit off its own owner mask | each flavour as its grid |
| a delink that names such a prim | "Unlink failed: no edit permissions.", with no key | nothing | each flavour as its grid |
| a linkset of 32 made physical | takes | takes | takes |
| a linkset of 64 made physical | refused: "Can't enable physics for object with physics resource cost greater than 32", keyed `CantEnablePhysCostTooGreat`, stating `MAX_OBJECTS` 32 | takes | each flavour as its grid; the fake grid counts a prim as a cost of one |
| the same refusal twice within a few seconds (**not held**) | the second comes **without its alert**: of refusals six seconds apart every other one was silent, of refusals twenty seconds apart none | — | every refusal is told |

Second Life's limit is on the set and not on a prim's distance from its
root: a root 48 m from each of two prims that stand 96 m apart links
neither. Two prims 64 m apart link whatever their size, so it is the
centres that are measured. Whether it is every pair of prims or the box
around them all was not told apart; the fake grid's Second-Life flavour
refuses a link that would put a prim it brings into one set with a prim
more than 64 m from it.

A refusal is whole on Second Life. `LinkFailed`'s own parameters count the
pieces it could have linked — one fewer than it was asked to, each time —
and it links none of them.

The reference viewer has no notification of the name `LinkFailed`, and
shows the alert's text as it came.

### Somebody else's prims

Measured on 2026-10-11 by the conformance case `object-link-stranger`. The
**owner** rezzes two cubes, `A` and `B`; the **stranger**, a second avatar
with no right over them, rezzes a third of its own, `X`. After each attempt
the owner asks the grid for all three again, so that what hangs off what is
read from the grid. Before each attempt a third of a minute passes without
a word, so that no alert goes unsent for having just been sent. It ran
twice on aditi and twice on the local OpenSim (`--secondary secondary`);
every row is checked on every run. The fake grid lets anybody edit anything
(`server-fake-grid-edit-permission-enforcement`), so these are the live
grids' answers alone.

Nothing linked that joined prims of two owners, and nothing a stranger sent
changed the owner's set, on either grid. OpenSim says nothing of any of it.

| attempt | Second Life | OpenSim |
| --- | --- | --- |
| the stranger links the owner's two prims | nothing links; "Link failed because you do not have edit permission.", keyed `LinkFailedNoPermToEdit` | nothing links; nothing is said |
| a link with the linker's own prim as the root and the other's under it — by the stranger, and by the owner | nothing links; "Link failed -- owners differ", keyed `LinkFailedOwnersDiffer` | nothing links; nothing is said |
| a link with the other's prim as the root and the linker's own under it — by either | nothing links; `LinkFailedNoPermToEdit` | nothing links; nothing is said |
| the stranger delinks a child of the owner's set, and then the whole set | the set stays; "Unlink failed: no edit permissions.", with no key | the set stays; nothing is said |

Second Life asks first whether the linker may edit the root, and only then
whose the rest are.

The fake grid used to hang a named prim off the root and leave the prims
under it where they were, free every child of a root that was delinked
alone, copy a root without its set, and send only the children of a link.
It keeps each set's order now (`link_sets`), numbers newcomers and
promotes a root as its flavour's grid does, sends a changed set whole and
in that grid's order, and refuses what Second Life refuses on the
Second-Life flavour. Its scripts still cannot read a link number:
`server-world-link-sets`.

Not measured: whether Second Life's reach is every pair's or a box's, and
whether it grows with prims larger than ten metres; the order a delink's
updates come in; whether a set's order survives a take and a rez; where a
seated avatar is numbered; a link of a prim the owner may not copy with
one it may not transfer (`LinkFailedCantLinkNoCopyNoTrans`), of an
attachment, of prims either side of a region's border, and by an owner's
friend with edit rights; whether Second Life's limit of 32 for a physical
set is a count or the cost it says; OpenSim's `LinksetPrims` set to
anything but its default of no limit.

## Contents

A prim holds an inventory of its own: scripts, notecards, objects. A viewer
reads it as a **listing** and writes it five ways — an inventory item
dropped in (`UpdateTaskInventory`), a script made new or dropped in
(`RezScript`), an item removed (`RemoveTaskInventory`), an item moved out
into the agent's inventory (`MoveTaskInventory`), and an item's asset saved
over a capability (`Update…TaskInventory`). None of the five is answered
with a message of its own. What says a write landed is the listing read
again, and what says a listing is stale is its **serial**: a number the
simulator moves on every change, carried in the `ReplyTaskInventory` and in
the prim's properties record and nowhere else.

Measured on 2026-10-12 by the conformance case `task-inventory`. One avatar
rezzes a cube on land it may build on and asks one question a leg; a second
stands by and is read after each. After every write the case listens for
the prim's record, for a `ReplyTaskInventory` nobody asked for, for the
prim sent again, for alerts and for inventory announcements, and then reads
the listing back over UDP. It ran five times on aditi (Mauve, the public
sandbox parcel) and five on the local OpenSim. Every row below is checked on
every run of both live grids and of both fake flavours, but for the ones
marked **not held**.

### The listing

| question | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the `RequestTaskInventory` capability | granted | not granted | `FakeSl` grants it, `FakeOpensim` does not |
| the UDP road: `RequestTaskInventory`, a `ReplyTaskInventory` naming a file, the file over `Xfer` | answered | answered | answered |
| an empty prim over UDP | serial 0 and **no file named** | the same | the same |
| the capability, `GET <cap>?task_id=<prim>` | `200`, a map of `inventory_serial` and `contents`, an array with no "Contents" folder in it | — | as Second Life |
| the same naming the serial the prim is at (`&inventory_serial=<n>`) | **`304`**, no body | — | as Second Life |
| the same naming another serial | `200` and the listing | — | as Second Life |
| a prim the region does not have, and no prim at all | `404` | — | as Second Life |
| one item of `contents` | `item_id`, `parent_id` and `asset_id` as ids; `name`, `desc`; `type`, `inv_type` and the sale type **by their short names** (`notecard`, `notecard`, `not`); `flags` and `created_at` as integers; `permissions` with the five masks as integers, four ids and `is_owner_group`; `sale_info`; and an empty `metadata` map | — | as Second Life |
| the two roads against each other, one prim, one moment | the same serial and the same items, field for field | — | the same |

The capability is how a current viewer reads a prim
(`LLViewerObject::fetchInventoryFromCapCoro`); UDP is what it falls back to,
and all OpenSim has. `Command::FetchTaskInventory` takes the capability
where the root region grants it and UDP otherwise — and after a capability
request that failed. It takes the serial of a listing the caller holds: a
prim still at it is answered `Event::TaskInventoryUnchanged` on both roads,
by the `304` on one and by a reply whose file is not fetched on the other.

### What a write does

| write | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a drop of an inventory item | a new item under a **new id**, the agent's item kept; its name, masks and asset the agent item's; the serial up by 1 | the same | the same |
| the same item dropped again | a second item, called `Name 1` | the same | the same |
| a script made new (`RezScript` naming no item) | lands under the name asked for and **runs**; the serial up by **2** | the same, and the serial up by 1 | each flavour as its grid; neither runs a script, and each says it runs |
| the same with the running flag off | lands **stopped**; the serial up by 1 | **runs anyway**; the serial up by 1 | each flavour as its grid |
| what a new script says (**not held**) | `Hello, Avatar!` | `Script running` | each flavour stores its grid's script and runs none |
| a script dropped out of the inventory | a new item under a new id, running; the serial up by 2 | the same, the serial up by 1 | each flavour as its grid |
| a script made in the inventory and never saved (**not held**) | names no asset there; dropped, it runs the grid's new script | has an asset from the start | has one on both flavours |
| a remove | the item goes; the serial up by 1, and by **2 for a running script**; nothing is filed | the item goes; the serial up by 1; **a copy of the item is filed in the agent's Trash**, announced with `UpdateCreateInventoryItem` | each flavour as its grid |
| a move of an item the agent may copy | a new item, under a new id and the name it had in the prim, in the folder asked for, announced with `UpdateCreateInventoryItem`; **the prim keeps its item and its serial** | the same | the same |
| a gesture saved over `UpdateGestureTaskInventory` | the item names the new asset, which is the one the capability's answer names; the serial up by 1 | the same | the same |
| a material saved over `UpdateMaterialTaskInventory` | the same | no such capability | each flavour as its grid |

The second step of Second Life's serial is the script **starting**: a
script that lands stopped takes one step, and removing one that runs takes
two.

### Who is told

The writer made each write three times: with both avatars holding the cube
selected, with only itself, and with nobody.

| what is sent | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the prim again, in a full update | to **every viewer that has it in sight, after every write** but a move | after a script lands and after a remove, to everybody; after a drop or a save, to nobody | each flavour as its grid |
| the prim's record, with the new serial | to every session **holding the prim selected**, the writer among them or not | to the **writer**, selected or not, and nobody else | each flavour as its grid |
| a `ReplyTaskInventory` nobody asked for | to every session holding the prim selected, **when a script has started** | never | each flavour as its grid |
| after a save over a capability | as after any write | to the saver, an alert — `Gesture updated`, `Notecard updated` — and no record | each flavour as its grid |
| after a move | nothing but the new item, to the mover | the same | the same |
| the serial the record of a script's landing states (**not held**) | one or two more than before: the record and the script's start race | one more | one more |

So a viewer that does not hold a prim selected is told of a write into it
by an object update alone on Second Life — which says nothing of contents —
and of another avatar's write by nothing at all on OpenSim unless a script
landed or something was removed.

### Writes that do not land

| write | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a remove of an item the prim does not hold | nothing | nothing | nothing |
| a move of one | nothing | nothing | nothing |
| a script naming an item the agent does not hold | nothing | nothing | nothing |
| a drop naming one | "Inventory creation on in-world object failed.", keyed `InventoryCreationInWorldObjectFailed` | nothing | each flavour as its grid |

In every one of them the serial stays where it was. Silence is the usual
answer on both grids, which is why a viewer cannot wait for a refusal.

### A script's experience

| question | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the `GetMetadata` capability | granted | not granted | `FakeSl` grants it, `FakeOpensim` does not |
| a script under no experience | `200` and an **undefined** value, not a map | — | as Second Life |
| an item the prim does not hold | `404` | — | as Second Life |

A script under an experience was not measured: the test avatars own none.
The fake grid answers with the experience the last save of the script
named.

### Somebody else's prim

Measured on 2026-10-12 by the conformance case `task-inventory-stranger`.
The **owner** rezzes a cube and puts a notecard and a running script in it;
the **stranger**, a second avatar with no right over it, reads and writes.
After each write the owner reads the listing back, and both are listened
to. It ran twice on aditi and twice on the local OpenSim
(`--secondary secondary`); every row is checked on every run. The fake grid
lets anybody edit anything
(`server-fake-grid-edit-permission-enforcement`), so these are the live
grids' answers alone.

| the stranger | Second Life | OpenSim |
| --- | --- | --- |
| lists the prim over UDP | answered: the owner's serial and items, under the owner's item ids, **every asset id nil** | **not answered** |
| lists it over the capability | `200`, the same listing, no asset ids | no such capability |
| asks a script's experience | `200`, undefined, as the owner is answered | no such capability |
| drops a notecard of its own | nothing lands; "Unable to edit this!", keyed `UnableEditItem` | nothing lands; nothing is said |
| makes a script | nothing lands; "Not permitted to edit this!", keyed `NoPermToEdit` | nothing lands; nothing is said |
| moves the owner's notecard into its own inventory | nothing arrives; "Not permitted to copy that inventory.", keyed `NoPermToCopyInventory` | nothing arrives; nothing is said |
| removes the owner's notecard | it stays; "Not permitted to edit this.", keyed `NoPermissionToEdit` | it stays; nothing is said |

The owner is told of none of it, on either grid: no record, no update, no
alert.

The fake grid used to drop a `RezScript`, a remove and a move without a
word, and answered a prim's listing over UDP alone. It lists a prim over
the capability now on the Second-Life flavour, makes, removes and copies
out items as its flavour's grid does, moves the serial as that grid does,
and tells the sessions that grid tells (`task_contents`). It runs no script
and keeps only whether each is running, which is what a viewer's Running
box reads; it does not clear a prim's "inventory is empty" flag
(`0x800`), which Second Life's full update after a first drop does.

Not measured: a move of an item the agent may not copy, which should take
it out of the prim (an owner cannot make itself one); a drop of a no-copy
item, which should take it out of the inventory; a script under an
experience; who is told when a notecard or a script in a prim is saved
over (`test-asset-save-mutation-survey`); a prim in a neighbouring region;
a friend with edit rights, and a group member with the power, in the
stranger's place; a listing of more than a few items, and the serial at
its sixteen-bit wrap.

## Touch and grab

A viewer has three messages for a mouse button on a prim: `ObjectGrab` at
the press, `ObjectGrabUpdate` while it is held and `ObjectDeGrab` at the
release — and three more for turning what it holds, `ObjectSpinStart`,
`ObjectSpinUpdate` and `ObjectSpinStop`. Neither grid answers any of the
six. What they do shows in what a script in the prim is handed
(`touch_start`, `touch`, `touch_end`, and the `llDetected*` values), and in
where the prim is afterwards: the same three messages are a **touch** of a
prim whose script takes one and a **drag** of a prim that has none.

Measured on 2026-10-12 by the conformance case `object-touch-grab`. One
avatar rezzes cubes on land it may build on, a second stands by, and the
two make one gesture a leg. The script is the probe: it says every event
it is handed, with what it detects, to its owner. It ran seven times on
aditi (Mauve, the public sandbox parcel) and eight on the local OpenSim
(YEngine). Every row below is checked on every run of both live grids, but
for the ones marked **not held**; the rows of *A prim with no script* are
checked on both fake flavours as well, which run no scripts.

### The events of a touch

| gesture | Second Life | OpenSim |
| --- | --- | --- |
| a press and a release at once | `touch_start`, **one `touch`**, `touch_end` | `touch_start`, `touch_end` |
| a press held for two seconds, nothing sent meanwhile | `touch` events **on the region's own clock**, 22.5 a second (45 in the two seconds, **not held**) | **no `touch` event at all** |
| a press, ten `ObjectGrabUpdate`s a tenth of a second apart, a release | the same 22.5 a second: an update adds none | **one `touch` an update**, ten |
| a release with no press before it | `touch_end` | `touch_end` |
| two presses and one release | the second press is **ignored**: one `touch_start` | **two `touch_start`s** |
| updates with no press before them | the first **opens a touch**: `touch_start`, and `touch` on the clock until a release | one `touch` an update, no `touch_start` |
| how many touches an event is handed, one avatar touching | 1 | 1 |
| the prim's click action set to *sit*, or to *disabled* | the events as before | the events as before |
| a script with no touch handler | no event, and nothing else | the same |
| what either avatar is sent after a touch | nothing | the prim in a **terse update, to everybody, for every `ObjectGrabUpdate`** |

So a held button is a held touch on Second Life whatever the viewer sends
between the press and the release, and on OpenSim only as far as the
viewer keeps sending updates. A click action is the viewer's business on
both: neither grid withholds a touch for one.

### What a script detects

| value | Second Life | OpenSim |
| --- | --- | --- |
| `llDetectedTouchFace`, `ST`, `UV`, `Pos`, `Normal`, `Binormal`, the message carrying a `SurfaceInfo` block | the block's values, **as sent**: a face a cube does not have (99), coordinates off the face (2.5, −1.5) and a normal three long come back unchanged | the same |
| the same, the message carrying none | face −1, `ST` and `UV` `<-1, -1, 0>`, the three vectors zero | the same |
| which message a `touch_start` reads | the `ObjectGrab` | the `ObjectGrab` |
| which message a `touch` reads | the **latest** message of the touch: the `ObjectGrab` until an update comes, then the update — an update with no block reads as no surface | the `ObjectGrabUpdate` it was raised for |
| which message a `touch_end` reads | the `ObjectDeGrab` | the `ObjectDeGrab` |
| `llDetectedGrab` in a `touch_start` | zero | **the `GrabOffset` of the `ObjectGrab`** |
| `llDetectedGrab` in a `touch` | the update's `GrabPosition`, less the prim's position, **less the `GrabOffsetInitial`** | the update's `GrabPosition`, less the prim's position |
| `llDetectedGrab` in a `touch_end` | zero | zero |
| `llDetectedKey` | whoever touched — the owner, or another avatar | the same |
| `llDetectedType` of an avatar standing still | 3 (`AGENT \| ACTIVE`) | 1 (`AGENT`) |

`llDetectedTouchPos` is documented in the region's coordinates and
`llDetectedTouchNormal` in the region's axes. Neither grid computes either:
a script reads what the viewer put in the block.

### The flags a viewer reads

| question | Second Life | OpenSim |
| --- | --- | --- |
| a prim with a running script that has a touch handler | `FLAGS_SCRIPTED` and `FLAGS_HANDLE_TOUCH`, in a full update to everybody as the script is saved | the same |
| the script saved again with no touch handler | `FLAGS_SCRIPTED` alone, again in a full update to everybody | the same |
| the script removed | neither flag, a full update to everybody | the same |
| a **child** of a linkset whose root has such a script | carries **both flags** itself — and is not sent again when the root's script lands: a viewer that does not ask keeps the flags it had | carries `FLAGS_SCRIPTED` only, and is sent again in full |
| a click action set by the owner | stored, and the prim sent in full to everybody | stored, and nobody told (§ Editing) |

A viewer that wants to know whether a click on a prim reaches a script has
to look at the prim's flags **and its root's**, on either grid.

### Which prim of a linkset is told

Two cubes linked, the other avatar clicking on the child. "root (link 2)"
is the root's script handed the event with `llDetectedLinkNumber` 2.

| scripts | Second Life | OpenSim |
| --- | --- | --- |
| a touch script in the root alone, a click on the child | root (link 2) | root (link 2) |
| the same, a click on the root | root (link 1) | root (link 1) |
| a touch script in both, nothing said of passing touches | child (link 2) alone | child (link 2) alone |
| the child's script having called `llPassTouches(PASS_ALWAYS)` (1) | child and root, each told link 2 | the same |
| `llPassTouches(PASS_IF_NOT_HANDLED)` (0) | child alone | child alone |
| `llPassTouches(PASS_NEVER)` (2) | child alone | **child and root**: any value but 0 passes |
| the child's script with no touch handler, and 0 or 1 | root (link 2) | root (link 2) |
| the child's script with no touch handler, and `PASS_NEVER` | **nobody** | root (link 2) |

### A prim with no script

The same messages sent at a cube nothing takes touches in.

| gesture | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a press and a release | nothing | nothing | nothing |
| a press, five `ObjectGrabUpdate`s asking a metre aside, a release, by the owner | **the prim is put where the update says the grab is**; a terse update to everybody | the same | the same |
| the five updates with no press before them | the same | the same | the same |
| the same by another avatar | the prim stays, and nobody is sent anything | the prim stays, and **everybody is sent a terse update for every update** | each flavour as its grid |
| a spin by the owner: five `ObjectSpinUpdate`s ending a quarter turn left | **the prim stands as the last update says**; a terse update to everybody | the same | the same |
| a second spin ending a quarter turn right | as that one says: the rotation is the prim's, not a turn added to it | the same | the same |
| a spin by another avatar | the prim stays, nobody is sent anything | the prim stays, a terse update | each flavour as its grid |
| a prim whose script takes touches, dragged by its owner | the prim **stays**, the script has its `touch_start`, `touch` and `touch_end`, nobody is sent anything | the prim stays, the same events, a terse update for every update | a prim flagged `FLAGS_HANDLE_TOUCH`, or under a root that is, stays on both flavours; neither runs a script |
| a prim whose script has no touch handler, dragged by its owner | moves, as a prim with no script | the same | the same |

This is the drag of the reference viewer's grab tool, and it is why a
viewer must not send an `ObjectGrabUpdate` for a click on a prim that
takes no touch: on either grid an owner's update carries the prim off to
wherever the update says the grab is.

### A physical prim

The cube made physical and left to come to rest; thirty updates asking it
half a metre aside, three seconds of them.

| gesture | Second Life | OpenSim |
| --- | --- | --- |
| a drag by another avatar | the prim stays | the prim stays |
| a spin by the owner, by steps to a quarter turn | the prim turns, and not to where the last update says | the same |
| a drag by the owner | the prim is **pulled away**: it was last seen some 80 m off, by the region's south-western corner, in every run | pushed a millimetre or two (0.08 m for a pull of two metres): a force of a tenth of the prim's mass times the distance, once an update |
| who is sent what meanwhile (**not held**) | terse updates, and now and then a full one | a terse update for every update |

Second Life's pull was not understood: the same messages that leave an
OpenSim cube where it lies sent Second Life's across the region, twice off
its edge. The census asks for half a metre and no more for that reason.

Not measured: a touch from further off than a few metres; a touch on an
attachment and on a HUD; two avatars touching at once, which is what an
event's count of touches is for; a press never released, by an avatar that
then logs out; `llDetectedTouchPos` and friends on a prim in a neighbouring
region; a drag of a prim its owner has locked, or one the everyone mask
lets anybody move; `llSetStatus(STATUS_BLOCK_GRAB)`; XEngine. What a
`touch` reads on OpenSim after a click or a hold with no update cannot be
asked: there is no such event there.

The fake grid runs no scripts, so it hands nothing a touch event: that, and
the detected values, are `server-world-touch-and-grab`. It moves and turns
a dragged prim as above (`sl-fake-grid/src/grabs.rs`), with one row in
`imitates.rs` for the one thing the grids differ in — OpenSim's terse
update for an update that moved nothing. A physical prim stays where it is
on both flavours: nothing there applies a force.

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
the pie takes it away again. A live run first deletes what earlier runs
of these tests left standing round the avatar, by the marker their prims'
names end in.

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

A link, by the same measurements:

- Link sends the selection's roots, the one selected last first, which is
  the root on both grids; Unlink names every prim of the selected sets, so
  that nothing is left for a grid to promote. Neither waits for an answer:
  the prims are drawn as the grid sends them again, hanging off what it
  says they hang off.
- A refusal is the grid's to word. `LinkFailed` — too far apart, too many —
  is a name no viewer has a notification for, and its text is shown as it
  came; `LinkFailedNoPermToEdit`, `LinkFailedOwnersDiffer` and
  `CantEnablePhysCostTooGreat` are shown from the viewer's own catalogue,
  the last with the grid's number in it. Where Second Life leaves the
  alert out of a refusal that follows another, and wherever OpenSim
  refuses, nothing is shown and the prims stay as they were.
- The link tool refuses a set of more than 256 prims itself, as the
  reference viewer does: Second Life's limit, and one a stock OpenSim
  region does not have (`viewer-link-limit-from-open-region-settings`).

Checked through the viewer automation (`e2e_objects`): on each fake
flavour, two prims are rezzed through the Build window, the second is
moved a hundred metres up, both are selected and Ctrl+L is pressed. On the
Second-Life flavour the viewer shows "pieces are too far apart" and draws
both prims free; on the OpenSim flavour it draws one hanging off the
other. The same test runs against a live grid with `SL_E2E_GRID=opensim`
and `=aditi`, and passes on both. The link and unlink chords themselves
are `e2e_live_checks`'.

A prim's contents, by the same measurements:

- The Content tab and the Object Contents window read a prim over the
  capability where the region grants it and over UDP where it does not, and
  over UDP again when the capability fails. A listing that never comes —
  the grid says nothing of a prim it does not have — ends the wait after a
  minute (`Event::TaskInventoryUnanswered`): the rows stop saying a change
  is on its way and show what the grid last listed.
- Nothing waits for an answer to a write, because there is none. The row
  says what was asked — "…adding", "…deleting", "…refreshing" — until the
  listing is read again, and the listing is what is shown. On Second Life
  the write goes over UDP and the listing over HTTP, and nothing orders the
  two: a listing that comes back at the serial the prim stood at is asked
  for once more a second later, naming that serial, and only the second
  answer — a `304`, or the same listing — is taken to mean the grid did not
  take the write. The reference viewer does the same.
- A `ReplyTaskInventory` nobody asked for, and a record with a serial the
  tab has not seen, each have the listing read again: that is how a script
  that has finished starting, and another avatar's write on Second Life,
  reach the tab.
- New Script makes a script that runs, as the reference viewer's does. It
  used to ask for a stopped one, which Second Life honours and OpenSim does
  not.
- The script editor asks which experience a task script is under when it
  opens one (`Command::RequestScriptExperience`), says so under the body,
  and names it again in the save. A save used to name none, which takes a
  script out of its experience. Where the region has no `GetMetadata`
  nothing is asked.
- A grid's alert is shown as the grid worded it.

Checked through the viewer automation (`e2e_objects`): on each fake
flavour, a prim is rezzed through the Build window and two scripts are made
in it from the Content tab, which shows the second as `New Script 1` — the
grid's name for it — and loses its row when it is removed. On the
Second-Life flavour the grid is then told the remaining script is under an
experience: the script editor, opened from the tab, says which, and the
save it sends names it. The grid then loses the prim without a word and a
third script is made, which nothing answers over either road: the row
saying it is being added goes, and the one script the grid last listed
stays. The same test, without the experience and the lost prim, runs
against a live grid with `SL_E2E_GRID=opensim` and `=aditi`, and passes on
both.

A touch, by the same measurements:

- A left click on a prim is a touch from the press to the release: an
  `ObjectGrab` when the pick lands, an `ObjectDeGrab` when the button is
  let go. It used to send the two back to back, so a script saw every
  press end at once and a held button was no touch at all on Second Life.
- For a prim whose script takes touches — `FLAGS_HANDLE_TOUCH` on it or on
  its root — an `ObjectGrabUpdate` follows the grab, and another whenever
  the cursor has moved to a different point of the prim's surface, or off
  it: when the reference viewer sends them, and all OpenSim hands a script
  a `touch` event for. Every one carries the surface under the cursor,
  since Second Life reads a `touch` from the latest message.
- For any other prim no update is sent at all, on either grid: an owner's
  update would carry the prim off.
- The surface a touch in the world states is in the region's frame — the
  point, the normal and the binormal. It used to be in the prim's own,
  which both grids handed a script as `llDetectedTouchPos` unchanged. A
  touch on a HUD still states the HUD's own frame.

Checked in the viewer's own world tests (a held button on a prim that
takes touches and on one that does not; the region frame of a world
touch), and through the viewer automation on each fake flavour
(`e2e_pilot`): a left click on a prim flagged as taking touches reaches the
grid as a grab, an update and a degrab, in that order, the grab stating a
point on the prim in region metres.

Not done in the viewer: a click on a prim that takes no touch still sends
the grab and the degrab, where the reference sends nothing
(`viewer-left-click-touches-a-prim-that-takes-none`); a click action is
not carried out and *disabled* does not stop a touch
(`viewer-left-click-click-actions`); the cursor does not say a prim takes
a touch (`viewer-touch-cursor`); and nothing drags or spins a prim
(`viewer-build-tool-row-parity`).

The fake grid neither returns somebody else's object, nor returns anything
by itself, nor ends a temporary prim: `server-fake-grid-object-return`. It
tells an agent of a return at once on both flavours, where OpenSim waits for
its next backup — 3.6 and 5.0 s in two runs, and past a four-second window
in three others — and puts several returns into one message.

Not measured: a rez of an item its owner may not copy, which should take the
item with it (a prim's owner cannot make itself one); who else in the region
is sent the kill, and in which form; a `DRD_SAVE_INTO_AGENT_INVENTORY` and
the save into a prim's contents; a
return by somebody else and an auto-return on Second Life; a rez onto a full
parcel or into a full region.
