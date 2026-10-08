# Avatars

A region tells a viewer about the other residents in it through two
channels that know nothing of each other. The **object stream** carries each
avatar as an object like any other — a full `ObjectUpdate` when it is new,
terse updates as it moves, a `KillObject` when it goes — with an
`AvatarAppearance` and an `AvatarAnimation` beside it. The **coarse feed**
(`CoarseLocationUpdate`) lists everybody in the region every second or so,
one byte to an axis. A viewer draws bodies from the first and its minimap and
radar from the second, and has to cope with a resident who is in one and not
the other.

Measured on 2026-10-08 by two conformance cases:

- `avatar-presence`, two residents standing together, one acting and the
  other only listening: three runs on aditi (in Ahern, a mainland region with
  three neighbours) and five on the local OpenSim (the north-eastern region
  of its 2×2 block; the first from the middle of the south-western one,
  which is roofed with test objects and let nobody climb);
- `parcel-privacy`, on OpenSim only: on aditi our avatars own no land
  (`gridspec-aditi-test-land`).

The fake grid shows its residents nobody but the scene's NPCs
(`server-fake-grid-agent-avatars-shared`), so its column speaks of the one
resident's own session and of those NPCs.

## The coarse feed

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| how often it is sent (**held**) | every 1.333 s, the same in all three runs | every 4.54 s | every 1.333 s on `FakeSl`, every 4.545 s on `FakeOpensim` |
| whether somebody moving changes that | no: 1.333 s while the other paced | no: 4.54 s | nobody moves |
| whom it lists | everybody in the region, the agent it is sent to included | the same; at most sixty (its source) | the scene's NPCs and the agent |
| where the agent's own entry stands | second of two in all three runs: the one who logged in first was first | first in one run and second in two: the order of its presence list | last |
| `You` (**held**) | the index of the agent's own entry | the same | the same |
| `Prey` before anything is tracked | absent (−1) | absent | absent |
| `Prey` after a `TrackAgent` for the other resident | still absent in every update of the next twelve seconds. The two are not friends, and no map rights were granted: with those it is not measured | absent: its encoder never sets it (the code that would is commented out) | absent |
| how a position becomes `X` and `Y` | **rounded** to the nearest metre: 7.64 is 8, 6.85 is 7, 8.35 is 8 | **cut off**: 127.82 is 127, 69.82 is 69 | each flavour's (`CoarseRounding`) |
| how a height becomes `Z` | a quarter of it, rounded: 235.74 m is 59 (236 m), 40.54 m is 10 | a quarter of it, cut off: 226.56 m is 56 (224 m), 29.19 m is 7 | each flavour's |
| a height above what a byte holds | 1,163 m is sent as 255 (1,020 m) | 1,177 m is sent as **0**: anything above 1,024 m is | each flavour's |
| a neighbouring region's feed | each of the three neighbours sends its own, down its child circuit, in its own region's coordinates | the same, from all three | none down a child circuit |

OpenSim's zero for a height it cannot state is the "height unknown" a viewer
already has to read in a neighbour's entry, so it costs nothing new. Our
viewer skips its own entry by `You` and by the agent's id, so the order of
the list does not matter to it.

## What announces an avatar, and what takes it away

The watcher stood still and the mover left and came back twice: by a
teleport to a neighbouring region and back, and by logging out and in again
at `last`. Times are from the moment the teleport was asked for, and for the
login from its XML-RPC answer.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| an avatar that does nothing | no object update in thirty seconds. Its `AvatarAnimation` is sent again five to seven times in that time, unasked | nothing at all | an NPC is sent once |
| an avatar pacing back and forth | six to eleven updates a second, as many as the mover is sent of itself | 1.3 to 1.9 a second, the same as its own. Seven a second when it paced into the test objects around the region's centre | nobody moves |
| leaving by teleport: the `KillObject` | 0.27 to 0.40 s after the request — before the mover's own arrival at 0.42 to 0.55 s | 0.19 to 0.24 s, with the mover's arrival | not shown |
| leaving by teleport: gone from the coarse feed | in its next update, 0.4 to 0.65 s | in its next, 0.4 to 2.6 s | not shown |
| a neighbour's avatar | sent down the neighbour's child circuit as a full object 0.44 to 0.57 s after the request, and listed in that region's coarse feed from 1.0 to 1.7 s | the same: the object at 0.2 s, the coarse entry with that region's next update | no avatars down a child circuit |
| coming back by teleport: the order | `AvatarAnimation` (0.17 to 0.19 s, while the mover is still on its way), the object (0.45 to 0.56 s), `AvatarAppearance` (0.49 to 0.62 s), the coarse entry (1.2 to 1.5 s) | the object, `AvatarAppearance` in the same instant (0.14 to 0.26 s), `AvatarAnimation` 30 ms later, the coarse entry with the feed's next update | not shown |
| how often each is sent in the fifteen seconds after | the appearance twice, the animations five to seven times | once each (the animations four times in one run of five) | once each |
| the neighbour's copy on return | killed at 0.26 to 0.39 s, before the object arrives in the watcher's own region | killed in the same instant the object arrives | not shown |
| logging out: the `KillObject` | 0.57 to 0.64 s after the `LogoutRequest` (which is answered at 0.17 to 0.20 s) | 5 to 15 ms | not shown |
| logging out: gone from the coarse feed | 0.7 to 1.6 s | in its next update | not shown |
| logging in: the order | the object 0.86 to 0.97 s after the login's answer, the appearance and the animations 45 to 65 ms later; the coarse entry within 50 ms either side of the object in two runs, 0.3 s after it in the third | the object and the appearance at 0.10 to 0.12 s, the animations 30 ms later, the coarse entry with the feed's next update | not shown |

A coarse entry may arrive before the object it belongs to (Second Life, at
a login) and outlive it by a second and a half (Second Life, at a logout),
so a viewer sees a resident it has no body for at both ends of a visit.

## How far an avatar is sent

The watcher dropped its draw distance to 64 m and the mover flew straight up
from beside it for seventy seconds, to about 1,150 m, and hovered there.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a draw distance of 64 m and an avatar climbing away | **never taken out of the stream**: no `KillObject` on the way to 1,123 m, nor in the seventy-five seconds after | the same, to 1,150 m | NPCs are not culled |
| updates of it meanwhile | about thirty in two and a half minutes: a steady climb is not reported, as the mover's own stream showed (`movement.md`) | 200 over the same time | none |
| an agent that **arrives** with a draw distance of 64 m while the avatar hovers 1,123 m above | sent the avatar's object 1.2 s after its arrival (the watcher went to a neighbouring region and came back) | sent it too, 1,150 m above, in the scene its arrival brings | sent every NPC |
| the coarse feed meanwhile | lists it throughout, at `Z` 255 | lists it throughout, at `Z` 0 | lists every NPC |

So within one region neither grid withholds an avatar for being far away,
whatever the draw distance: the draw distance decides which *neighbouring
regions* an agent holds ([Teleport](teleport.md), *Neighbours and
crossings*), and an avatar in a region the agent holds no child circuit to
is not sent at all. Because nothing was ever taken away, the legs that
would have followed — what a larger draw distance or a camera beside the
avatar brings back, and at what distance it returns — did not run on either
grid.

This is not what `viewer-near-avatar-stuck-coarse-sphere` assumed. That
report, of a resident 622 m up who stayed a coarse dot, was put down to the
draw distance; a same-region avatar almost twice as far off was sent to an
agent with an eighth of that draw distance. What kept that resident out of
the object stream is open again.

## Parcel privacy

A parcel's owner can turn off *avatars on other parcels can see and chat
with avatars on this parcel* (`SeeAVs`, settable over the
`ParcelPropertiesUpdate` capability only). `parcel-privacy` chops a 64 m
corner off the region's parcel, turns it off there, and moves two residents
in and out of it one at a time.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the edit | not measured: no land | taken over the capability; the parcel reads back `SeeAVs` false | stored and read back; nothing is hidden |
| one resident inside, the other outside (**held**) | not measured | the one outside is sent a `KillObject` for the one inside 60 ms after it steps in | not shown |
| the same, seen from inside | not measured | the one inside **keeps** the one outside in its stream: hiding is one-way | not shown |
| both inside (**held**) | not measured | sent each other again, as full objects, 70 ms after the second steps in | not shown |
| stepping out | not measured | sent again 60 ms after | not shown |
| the coarse feed | not measured | lists both to both at every step: the hidden avatar stays on the map | not shown |
| the owner | not measured | hidden like anybody else: the owner standing outside its own parcel lost the resident inside it | not shown |

OpenSim's source exempts a viewer in god mode and nobody else.

## What the fake grid does about it

`sl-fake-grid` now sends the coarse feed on each flavour's timer, as part of
the region's telemetry: the scene's NPCs where they stand and then the agent
where it last arrived (it tracks no walking), each cut down to an entry the
way the flavour's grid does it, with `You` on the agent's own. `avatar-presence`
runs offline against both flavours for that much and holds them to the
interval, the self-listing and `You`.

Everything that needs a second resident — the object stream of one, the
order of an arrival, the departure's `KillObject`, a neighbour's avatars
and coarse feed down a child circuit, parcel hiding — waits for residents to
be shown to each other (`server-fake-grid-agent-avatars-shared`), which is
built to this chapter.

## What our viewer does with it

It keeps the two channels apart, which is what both grids need. A resident
in the object stream is a body and an exact radar position; one only in the
coarse feed is a placeholder and an approximate position; and an avatar
whose object is killed while the feed goes on listing it — a resident who
stepped onto a hidden parcel, or left for a neighbouring region the viewer
holds — falls back to the placeholder with the feed's next update and is a
body again when the object returns. `e2e_two_avatars` checks that round
trip on the radar
(`a_resident_out_of_sight_stays_listed_coarsely_and_returns`), beside the
existing checks of an arrival and of a neighbour's coarse-only resident.
Against the live grids, two of our viewers list each other on the radar and
one offers the other a teleport from its row, on OpenSim and on aditi
(`a_live_teleport_offer_is_declined_in_silence_and_then_taken`).

Not checked by a test: the placeholder at a coarse height of zero, which is
what OpenSim sends above 1,024 m.
