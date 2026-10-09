# Objects

## Updates

A simulator tells a viewer about the objects round it in five messages.
`ObjectUpdate` spells every field of an object out;
`ObjectUpdateCompressed` packs the same into one blob;
`ObjectUpdateCached` names an object by its id and a checksum, for a viewer
to answer from its cache or ask for in full with `RequestMultipleObjects`;
`ImprovedTerseObjectUpdate` carries motion alone; and `KillObject` says an
object is gone from the viewer's view. A viewer has four things to say
about what it wants: the `Flags` of its `RegionHandshakeReply`, the draw
distance and camera of its `AgentUpdate`s, and — on Second Life — the mode
of the `InterestList` capability.

Measured on 2026-10-09 by two conformance cases. `object-update-decode` arrives,
then changes one thing at a time and watches the stream for 20 s after each: the
draw distance down to 32 m, the camera to the far corner of the region looking
out of it, the interest-list mode to `360` and back, the camera back, the draw
distance back to 256 m. `object-handshake-flags` logs in once for each of five
combinations of handshake flags and watches each arrival for 25 s. Both read
every message off `Event::ObjectStreamBatch`. The first ran four times on aditi
and three on the local OpenSim, the second twice on each: aditi in Ahern, a
mainland region of some 1,350 objects with three neighbours, the agent in its
south-west corner; OpenSim in a region of 129 objects, the agent in the middle.
The first runs of each were made with handshake flags of zero and the later ones
with the flags the client now sends; the tables say which where it matters. Rows
marked **held** are checked on every run of a live grid; **held everywhere** on
the fake flavours too.

### The arrival

Both grids send every message of the stream reliably.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a region's prims, trees and grass (**held**) | `ObjectUpdateCompressed`, four to a message as a rule (one to seven), 137 to 1,186 bytes | the same, five to a message as a rule (three to seven), 417 to 773 bytes | full `ObjectUpdate`s |
| avatars, and a few prims | full `ObjectUpdate`: 24 objects in five messages | the agent's own avatar alone | every object |
| motion | `ImprovedTerseObjectUpdate` of the agent's avatar, once | the same, three times | none unasked |
| when | 2.0 to 2.3 s after the first datagram until 6.1 s | 0.1 to 1.3 s | with the rest of the arrival |
| which objects | those within the draw distance: 1,050 to 1,060 of the region's 1,350 at 256 m from its corner | all 129 | all |
| a neighbour's objects | down its child circuit, in the same forms: some 2,150 of three neighbours' | the same: five | all of each |
| kills during the arrival | 300 to 375 entries naming 150 to 200 objects the viewer was never sent, most of them twice | none | none |

### What the handshake reply's flags change

The reference viewer always sets bit 2 ("I understand an appearance
message about my own avatar"), sets bit 0 ("send all cacheable objects")
while its object-cache culling is on, and sets bit 1 ("my cache is empty")
when it has no cache file for the region.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| neither cache bit (**held**) | every object is first named in an `ObjectUpdateCached`, up to 98 to a message: 1,209 in 28 to 30. The viewer asks for what it does not hold and is sent 1,050 of them; the other 150 or so, beyond the draw distance, are killed instead | the same probe first: 129 objects in eight messages of up to 45, and every one sent when asked for | no probes |
| bit 1, cache empty (**held everywhere**) | no probes: the same objects come compressed at once | the same | no probes |
| bit 0, send all | the probes name 1,356 objects of the agent's region instead of 1,209, and all of them are sent; a neighbour's 11,485 instead of 3,346. No kills in the agent's region | nothing changes | nothing changes |
| bits 0 and 1 | 1,180 objects at once, and 2,280 kill entries down the child circuits | as bit 1 | as bit 1 |
| the agent's own `AvatarAppearance` at arrival (**held everywhere**) | only with bit 2 | whatever the flags | `FakeSl` only with bit 2, `FakeOpensim` always |

OpenSim reads bit 1 and nothing else (`LLClientView.m_viewerHandShakeFlags`).

### Draw distance and camera

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the draw distance taken from 256 m to 32 m (**held**) | 236 objects killed in twelve messages, 0.4 to 2.8 s later | nothing | nothing |
| what a kill names (**held**) | a linkset's root, and never a prim under it: the 236 kills took 741 child prims with them unnamed | — | — |
| the draw distance brought back (**held**) | the same 888 objects again, 670 of them child prims, compressed, within 4.5 s; down the child circuits 2,854 more | nothing: no object had gone | nothing |
| the camera taken 340 m from the avatar at a 256 m draw distance | 29 objects more, and two killed | nothing | nothing |
| the same at 32 m | one object killed | nothing | nothing |
| the camera brought back | 54 objects sent again, one of them new | nothing | nothing |

So Second Life's list is drawn round the agent, out to the draw distance,
and reaches a little further where the camera is; OpenSim sends a region's
objects whatever the viewer says of its range.

### The interest-list mode

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the `InterestList` capability (**held everywhere**) | granted | not granted | `FakeSl` grants it, `FakeOpensim` does not |
| a POST of `{mode: "360"}` (**held everywhere**) | `200`, `{mode: "360", previous_mode: "default"}` | — | the same |
| a POST of a mode it does not know | `200`, `{mode: "default", previous_mode: …}` | — | the same |
| a DELETE | no answer within 30 s | — | `405` |
| what the stream then carries | nothing different, in either mode, at this spot: with the camera 340 m off and looking away, at 32 m and at 256 m, no object came or went for the switch | — | nothing |
| with neither cache bit set | the switch to `360` was followed by a probe of every object in range (1,035 entries, all of them held), and the switch back by 942 | — | — |

The default list was not seen to leave out what the camera does not face:
turning it away killed one or two objects of a thousand. What `360` adds
where a default list does cull by view was not found on this region.

## What our viewer does with it

- It tells both grids that its cache is empty and that it understands its
  own appearance (`RegionHandshakeReplyFlags::CLIENT_DEFAULT`). It used to
  send flags of zero, which cost a probe and a request for every object of
  every region on both grids, and on Second Life left an arriving agent
  without its own `AvatarAppearance` until something else brought one. It
  does not ask for all cacheable objects: that is for a viewer that culls a
  whole region for itself.
- A kill takes with it every prim under the object it names
  (`Event::ObjectRemoved` for each), as the reference viewer's
  `markDead` does, and leaves an avatar seated on it. Before this
  measurement a linkset out of range on Second Life left its child prims
  behind: rootless in the scene, and answering the probes the simulator
  sent when the linkset came back, so that only the root was fetched again.
- `Command::SetInterestListMode` posts the mode where the region grants
  the capability, the session remembers it, and both runtimes ask again of
  each region the agent moves into, as the reference viewer does. The 360°
  capture switches to `360` while it looks around and back when it has its
  faces, and waits two seconds before its first face for the answer to
  arrive.
- `Event::ObjectStreamBatch` carries each message's form and the objects
  it named, for a client that wants to know how it was told.

Checked through the viewer automation (`e2e_objects`), on each fake flavour
and on each live grid (`SL_E2E_GRID=opensim` and `=aditi`):

- the viewer's world holds its region's objects once the arrival is over,
  and its own avatar's node comes to hold the bakes of an appearance — which
  on Second Life it is only sent because of what its handshake reply says;
- with the draw distance taken to 32 m through the viewer's own setting,
  Second Life's out-of-range objects leave the viewer's world and none that
  stays hangs off a root that went; back at 256 m they are there again. On
  OpenSim and on the fake grid the same moves change nothing in the agent's
  region.

The fake grid sends neither compressed nor cached updates, culls nothing
and does nothing about the interest-list mode but answer. Making it send
each grid's forms is `server-fake-grid-object-update-forms` and
`server-world-update-scheduling`.

Not measured: a viewer that answers probes from a cache of its own (ours
keeps none between sessions); what a kill names when an object is deleted
or taken, which is `gridspec-object-rez-derez`'s; an interest list that
culls by view.

## Properties

An object update says where an object is and what it looks like. Who made
it, who owns it, what it is called and what may be done with it is its
**record**, and a simulator sends that in two messages of their own:
`ObjectProperties`, the answer to an `ObjectSelect`, and
`ObjectPropertiesFamily`, the condensed answer to a
`RequestObjectPropertiesFamily`, which needs no selection.

Measured on 2026-10-09 by two conformance cases. `object-properties` rezzes
a cube as one avatar, with a second avatar beside it, and asks each question
below in a leg of its own, listening for four seconds after each for the
answer or for the silence that is one; it ran six times on aditi (Mauve, the
public sandbox parcel) and eight on the local OpenSim. `object-select-scene`
selects what a region already holds — eight root prims, eight child prims,
eight prims of a neighbouring region and the agent's own avatar — and ran
twice on aditi (Ahern) and twice on OpenSim. Every row below but the ones
marked **not held** is checked on every run of both live grids and of both
fake flavours.

The survey had it that aditi never answered an `ObjectSelect`. It answers
every one that names an object.

### A select

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a select of an object the agent owns | its `ObjectProperties`, 0.2 to 0.35 s later, and an `ObjectPhysicsProperties` over the event queue | the same, 0.02 s later | the same |
| of somebody else's object, an avatar beside it | the same, and the same record field for field | the same, with a full `ObjectUpdate` of the object ahead of it (`SelectPrim`: "if a friend got or lost edit rights after login, a full update is needed") | each flavour as its grid |
| of an object already selected | answered again, in full | the same | the same |
| a deselect | a terse update of the object | nothing | each flavour as its grid |
| of eight root prims in one message (none of them the agent's) | eight records in two messages of four, or of five and three | eight records in three messages of three, three and two, 15 to 30 ms apart | eight records, one to a message (**not held**) |
| of eight child prims | eight records, each the child's own | the same | the same |
| of eight prims of a neighbouring region, asked of that region | eight records, from the neighbour's simulator | the same (five of five) | the same |
| of the agent's own avatar | nothing | nothing | nothing |
| of a local id the region does not have | nothing | nothing | nothing |
| of a linkset's root alone | the root's record and no child's | the same | the same |

OpenSim's three to a message is its throttle and not a packet's size. It
turns queued records into messages at most every 20 ms
(`LLUDPClient.MIN_CALLBACK_MS`), each time as many as the task throttle lets
through in 30 ms (`HandleQueueEmpty`), counting a record as 200 bytes
(`ProcessEntityPropertyRequests`). At the task rate a client that sets no
throttle is given, 18,500 bytes a second, that is 555 bytes: three records.
A client that asks for a higher task throttle is sent more to a message, up
to the 1,200 bytes one may hold. Why Second Life's two messages split as
they did was not found: they came in the same millisecond.

So a viewer that wants a linkset's children named selects every prim of it,
as the reference viewer does, and one that is not answered is looking at
something that is not an object.

The physics record is the same on both for a cube nobody has changed:
shape type 0, density 1000, friction 0.6, restitution 0.5, gravity
multiplier 1.

### The record of a new prim

| field | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| name, description, touch and sit names | `Object`, and three empty strings | the same | the same |
| creator and owner | the rezzer | the same | the same |
| last owner | nobody | the rezzer | each flavour as its grid |
| group | none | none | none |
| creation date | **microseconds** since the epoch | the same | the same |
| base and owner masks | `0x7fffffff` | `0x0009e000`: transfer, modify, copy, export and move, which is all OpenSim defines | each flavour as its grid |
| group and everyone masks | `0` | `0` | `0` |
| next-owner mask | `0x00082000`: move and transfer | the same | the same |
| ownership cost | 10 | 0 | each flavour as its grid |
| sale type and price | not for sale, at a price of 10 | not for sale, at 0 | not for sale, at 0 (**not held**: a price is not read off a record that is not for sale) |
| category, contents serial, the three aggregate-permission bytes | 0 | 0 | 0 |
| item, folder and source task | nil | nil | nil |
| texture ids | one for each face, the same id six times for a cube | none ("still not sending, not clear the impact on viewers") | none (**not held**) |

Our own type said the creation date was in seconds. Nothing of ours read it.

### The family record

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a request about an object | one `ObjectPropertiesFamily`, its request flags echoed (0, and the pay dialog's 4), agreeing with the full record in every field the two share | the same | the same |
| from an avatar that does not own it | the same | the same | the same |
| about a child prim | the **root's** record, under the root's id | the same | the same |
| about an id nothing has | nothing | nothing | nothing |

### Who is told of a change

The primary avatar made each change three times: with both avatars holding
the cube selected, with only itself, and with nobody.

| change | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a name or a description (`ObjectName`, `ObjectDescription`) | the record, to the session that made the edit, selected or not; to nobody else | to nobody (`SceneGraph.PrimName` stores it and sends nothing) | each flavour as its grid |
| a price or a permission (`ObjectSaleInfo`, `ObjectPermissions`) | the same: the editor, selected or not, and nobody else | the same | the same |
| something written into its contents (`UpdateTaskInventory`) | the record, with its contents serial advanced, to **every session holding it selected** — the writer if it is one of them, and not otherwise | to the writer, selected or not, and nobody else | each flavour as its grid |
| an edit made with nothing selected | takes | takes | takes |
| a link | the record of each child that came under the root, to the linker | the root's record, to the linker | each flavour as its grid |

A selection is therefore a subscription on Second Life alone, and to one
thing: what is in the prim. An avatar looking at a prim somebody else
renames, prices or re-permissions is not told on either grid, and shows the
old record until it selects the prim again. Nothing arbitrates two editors.

### A child prim's record

| field | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| name | the child's | the child's | the child's |
| permission masks | the root's | the root's | the root's |
| sale type and price | the child's own | the root's | each flavour as its grid |

### What our viewer does with a record

- The Build window's General tab reads the name, the description, the creator,
  the owner, the group and the permission boxes off the record of the primary
  selection, and now keeps the fields that would *write* one — the name, the
  description, the permission boxes — shut until the record has come (the
  group's Set… button reads nothing off it and stays live). They used to be live
  over a blank record for the third of a second Second Life takes to answer, and
  for good over a select that is never answered. The rest of the window reads
  the object update and never waited.
- A commit writes the field at once and sends the edit. Where the grid
  answers (Second Life always; OpenSim for a price or a permission, after
  which the viewer selects again to read what was kept) the answer
  replaces what was written; where it does not, the field keeps it, and a
  fresh selection reads what the grid holds.
- The Content tab re-reads a prim's contents when a record arrives with a
  contents serial it has not seen — which is how it hears of another
  avatar's write on Second Life. On OpenSim it does not hear of one.
- `ObjectProperties::creation_date` is documented as what it is.

Checked through the viewer automation (`e2e_objects`): on each fake flavour
a freshly rezzed prim's name reaches the General tab and opens the field, a
rename shows and is what a fresh selection reads back, and a select the grid
does not answer leaves the name blank and shut; and on each live grid
(`SL_E2E_GRID=opensim` and `=aditi`) the same rez, record, rename and fresh
read against the grid's own answers.

Not imitated by the fake grid: how many records go to a message
(`server-fake-grid-object-record-batching`, deferred until a test needs it), the
texture ids of Second Life's record
(`server-fake-grid-object-record-texture-ids`), and its price of 10 on a prim
that is not for sale. Not measured: a category edit's answer (no viewer sends
one); what a group or an owner change is answered with; who is told when a
script or a notecard *in* a prim is saved over, which is
`test-asset-save-mutation-survey`'s; a select across a region border of an
object the agent may edit.
