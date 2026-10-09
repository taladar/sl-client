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
