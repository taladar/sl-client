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

## Effects and sounds

Two things a viewer sends are meant for the *other viewers* and not for the
simulator: a `ViewerEffect` (a gaze, a pointing, the editing beam, the
spheres and spirals) and a `SoundTrigger` (the sound of a gesture). A region
answers neither. Its whole part is to decide who is sent which, and that is
where the two grids differ in nearly everything. A script's sounds reach a
viewer in four more messages and in the prim's own update.

Measured on 2026-10-13 by the conformance case `effects-sounds`: one avatar
sends, a second one beside it listens, then climbs away in three steps (16,
40 and 125 m). Six runs on aditi (Mauve; the first two wholly on the
sandbox parcel, which bars avatar sounds, the rest on a parcel that lets
everybody's play and at the end on the sandbox's) and seven on the local
OpenSim, one of them as the land's owner. No answer changed between two
runs at the same spot, and each run is now held to 157 of them. A
probe script in a cube makes the script's sounds: it does what its owner
says on a channel.

### Viewer effects

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| who is sent an effect (**held**) | everybody, **the sender too** | everybody but the sender | as the flavour's grid |
| which types (**held**, all twenty and two that are none) | every type but floating text (type 0), the unknown ones included | every type | as the flavour's grid |
| how far (**held**) | at 122 m as at 3 m: no range | no range — but a message of **nothing but gazes and beams** only to a viewer whose **camera** is within 10 m of the sender's avatar: lost at 16 m, and back at once with the camera put beside the sender and the avatar left where it hovers | the same rule, by where the session's camera last was |
| the agent an effect names (**held**) | made the sender, whoever the viewer named: the other avatar, a stranger, nobody | passed on as the viewer wrote it | as the flavour's grid |
| the payload (**held**) | a gaze, a pointing and a beam have to **begin with the sender's id**: one of the other avatar's, one of three bytes, of none, and a beam of 255 bytes are dropped (a gaze with a byte too many is passed on); a sphere is passed on with no payload and with 255 bytes | passed on, whatever it is | as the flavour's grid |
| duration and colour (**held**) | as sent: no time, minus five seconds, a billion | as sent | as sent |
| three effects in one message (**held**) | one message of three | one message of three | one message of three |
| the same effect twice, twenty in a second (**held**) | each passed on | each passed on | each passed on |

OpenSim's source names its two exceptions *discardable*
(`Scene.ProcessViewerEffect`): a message is held back only when every effect
in it is a `LookAt` or a `Beam`. That is why a pointing and a sphere still
arrive from 125 m. Both grids send effects down a child circuit as well
([Teleports](teleport.md)).

### A viewer's sound

A viewer states the sound, a gain, a region handle and a position, and
leaves the owner, the object and the parent nil.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| who is sent it (**held**) | everybody, the sender too | everybody **within 100 m of where the sound is said to be**, the sender too | as the flavour's grid, by where each avatar last arrived |
| how far (**held**) | stated 15, 40, 90, 110 and 150 m above the sender, and heard by a listener 121 m off: always passed on | stated 15, 40 and 90 m up: passed on; 110 and 150 m up, and to a listener 126 m off: not | as the flavour's grid |
| a position outside the region (**held**) | passed on | not (it is more than 100 m from anybody) | as the flavour's grid |
| whose it says it is (**held**) | owner and object the sender, **parent nil** | owner, object **and parent** the sender | as the flavour's grid |
| gain 2.5, −1 and 0 (**held**) | passed on as stated | passed on as stated | passed on as stated |
| a nil sound, a sound nobody has (**held**) | passed on | passed on | passed on |
| the handle of another region (**held**) | passed on with that handle | passed on with that handle | passed on with that handle |
| ten in a second (**held**) | all ten | all ten | all ten |
| on a parcel that **bars avatar sounds** | nobody is sent it, not even the sender (the sandbox parcel, `AnyAVSounds` false and our avatars not in its group; **held**) | the same (measured once as the land's owner, who turned both switches off and on again) | nobody is sent it where the sender's parcel says `any_av_sounds` false; group membership is not modelled |

A region does not look at what it passes on. The listener has to: clamp the
gain, skip a nil sound, and place a sound by the handle it states.

### A script's sounds

What the two avatars were sent for each call, the owner and the other alike
unless said otherwise. *update* is a full `ObjectUpdate` of the prim, with
its sound fields.

| call | Second Life | OpenSim |
| --- | --- | --- |
| `llTriggerSound` (**held**) | `SoundTrigger`: owner, object the prim, **parent nil**, at the prim | the same with **parent the prim** (it is its own root) |
| `llPlaySound` (**held**) | `AttachedSound`, flags none — and an update where the prim's flags still said *stop* | `AttachedSound`, flags none |
| `llLoopSound` (**held**) | `AttachedSound` flags *loop* **and** an update: the sound, gain, *loop*, radius 0 | an update alone: the sound, gain, *loop*, **radius 100** |
| `llLoopSoundMaster`, `llLoopSoundSlave` (**held**) | the same with *loop\|master*, *loop\|slave* | the same in its update |
| `llPlaySoundSlave` (**held**) | `AttachedSound` flags *slave\|pending* | flags *slave* |
| `llAdjustSoundVolume` (**held**) | `AttachedSoundGainChange` and an update at the new gain | `AttachedSoundGainChange` |
| `llStopSound` (**held**) | `AttachedSound` with a nil sound, flags *stop*, and an update: no sound, *stop*, no owner | an update: no sound, *stop*, the owner still named |
| `llPreloadSound` (**held**) | `PreloadSound`, owner the owner | `PreloadSound`, **owner the prim** |
| `llSetSoundQueueing(TRUE)` (**held**) | an update, flags *queue\|stop* | nothing; the next `llPlaySound` carries *queue* |
| `llSetSoundRadius(5)` (**held**) | an update, radius 5 | nothing; the next loop's update carries radius 5 |
| `llTriggerSoundLimited`, box around both (**held**) | `SoundTrigger`, gain 0.50 | `SoundTrigger`, gain 0.49 |
| the same, box elsewhere (**held**) | nothing | nothing |

| how far a script's sound is sent (**held**) | Second Life | OpenSim |
| --- | --- | --- |
| `llTriggerSound` | at 15 and 36 m; not at 121 m | at 17 and 40 m; not at 126 m |
| `llPlaySound`, `llPreloadSound` | at every step | at every step |

| an arrival beside a looping prim (**held**) | Second Life | OpenSim |
| --- | --- | --- |
| what says so | the prim's full update: the sound, gain, *loop* | the same |
| a sound message | none | none |

Neither grid sends an arriving viewer a `PreloadSound` or an
`AttachedSound`: a loop that is already running is on the prim.

**Not measured:** a sound in a neighbouring region (whether a `SoundTrigger`
goes down a child circuit); what a parcel's *group only* switch lets through
for a member; `llSetSoundRadius` and a listener beyond it; collision sounds;
a script's sound from a child prim of a linkset.

### What the fake grid does about them

`sl-fake-grid` passes a viewer's effects and sounds on by its flavour's
rules (`relays.rs`, `ImitatedGrid::relay_policy`), the way it routes local
chat: the session that received them publishes them to the region and every
session's watcher asks the policy whether its own avatar is sent them.
`effects-sounds` runs its viewer legs offline on both flavours and holds
them to 95 of the answers above. Because the fake grid tracks no walking,
distance is from where an avatar last arrived, and the camera rule from the
last `AgentUpdate`; those two are checked by unit tests of the policy. A
timeline can show a session effects and sounds of its own making
(`Action::Effects`, `TriggerSound`, `AttachSound`, `AdjustAttachedSound`,
`PreloadSounds`). No script runs, so nothing in the second half of this
section happens there (`server-lsl-lib-comms` and its siblings).

### What our viewer does with them

It turns another avatar's gaze into a head turn and its pointing into a
reach, and ignores an effect whose source is itself — which is what Second
Life's echo of every effect needs. It draws no beam, sphere or edit sparkle
(`viewer-viewer-effect-render`), and of its own it sends a pointing only: no
gaze and no beam (`viewer-lookat-faithful`). A gaze it does send will be
dropped by Second Life unless its payload names the agent itself.

Sounds from both roads go through one place, as in the reference: a
`SoundTrigger` is muted by owner or object, clamped by the mixer and placed
by its region handle, and since this measurement a trigger with a nil sound
asks for nothing. A loop that arrives as an `AttachedSound` *and* in the
prim's update, as Second Life sends it, starts once; `llStopSound` ends it
in either grid's form; and a prim's update that names no sound leaves a
one-shot playing, which Second Life's update after an `llPlaySound` relies
on. All of that is unit-tested in `world_sounds.rs`. The viewer plays no
gestures yet (`viewer-gesture-runtime`), so it triggers no sound itself;
when it does, the sound has to come from the grid's echo, which both grids
send, and it will be silent on a parcel that bars avatar sounds.

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
