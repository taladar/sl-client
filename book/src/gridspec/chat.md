# Local chat

A viewer's `ChatFromViewer` and a script's `llSay` family come back as the
same message, `ChatFromSimulator`: a type, a source and its kind, an owner, a
name, how audible the line is, where it was said, and its text. A region
answers the speaker nothing else. What it does with a line shows in who is
then sent it.

Measured on 2026-10-13 by two conformance cases.

`local-chat` has avatar **A** speak, own a probe prim and move away, and
avatar **B** stay and give the probe its orders: four runs on aditi (Mauve,
on a parcel that shows its avatars) and four on the local OpenSim. A climbs
in three steps, to 13–19 m, 37 m and 122 m from B. No answer changed between
two runs, and every run is held to 112 answers, and to 61 more where a
script is needed to ask. The probe script says what it is told to on a
channel and says when it has.

`chat-border` stands the two avatars either side of a region border (Ahern's
western one on aditi, the Default Region's eastern one on OpenSim), 8–12 m,
36–40 m and 117–122 m apart: four runs a grid, held to 32 answers.

## What a line says of itself

Both grids, a viewer's line heard by the other avatar and by the speaker,
who is sent its own line back:

| field | a viewer's line | a script's line |
| --- | --- | --- |
| type | the type the viewer wrote | whisper, say, shout by the function; `owner` for `llOwnerSay`; `direct` for `llRegionSayTo`; `debug` for anything on the debug channel |
| source | the agent | the prim, as an object |
| owner | the agent again | the prim's owner (but see OpenSim below) |
| name | the avatar's legacy name | the prim's name; an empty name where the prim has none |
| audible | fully | fully |
| position | where the avatar stands | where the prim stands (but see OpenSim below) |

**A line is never marked less than fully audible, on either grid.** Out of
range it is not sent at all. The one message that is marked *barely audible*
is Second Life's typing notice ([Typing](#typing)).

## A viewer's line

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| how far a whisper, a say and a shout carry (**held**) | 10 m, 20 m, 100 m: a whisper unheard at 12.6 m, a say heard at 18.7 m and unheard at 36.5 m, a shout heard at 37.6 m and unheard at 121.5 m | the same three | the same three |
| measured between (**held**) | the two avatars. A whisper is unheard at 12.6 m with the speaker's camera beside the hearer, and with the hearer's beside the speaker | the same | the avatars, where each last arrived |
| the ranges as the region states them (**held**) | `RegionInfo` carries the chat block: whisper 10 m, say 20 m, shout 100 m, offsets 0, flags 0 | `RegionInfo` carries none ([`SimulatorFeatures`](region-arrival.md) states them) | as the flavour's grid |
| a line of a type no viewer sends — 3, `debug`, `region`, `owner`, `direct`, 10, 255 (**held**) | dropped: nobody is sent it, not the speaker | passed on as sent to **the whole region**, the `owner` and `direct` types to everybody: heard at 126 m | as the flavour's grid |
| a say on channel 1, −1, 2 000 000 000 and the lowest there is (**held**) | no avatar is sent it | no avatar is sent it | no avatar is sent it |
| … and a script listening there | hears channel 1; **not channel −1** | hears both | as the flavour's grid |
| a line on the debug channel (**held**) | passed on **as the type it was sent as**, as far as that type carries: a say heard at 12.6 m and unheard at 36.5 m. A listening script hears it | passed on **as a debug line** to the whole region, whatever it was sent as: heard at 126 m. A listening script hears it | as the flavour's grid |
| a line with no text (**held**) | passed on, empty | passed on, empty | passed on, empty |
| a long text (**held**) | the text and the speaker's name together come to **1151 bytes** at most: 1100 bytes arrive whole, 2000 and 3000 are cut — one byte shorter for the avatar whose name is one byte longer — and never inside a character | cut to **1000 characters**, and what is sent of those to 1023 bytes: 1023, 1024, 1100, 2000 and 3000 bytes arrive as 1000; 1200 bytes of two-byte characters as 1022 | as the flavour's grid |
| a line break and a tab, spaces at the end, an emote (**held**) | as sent | as sent | as sent |
| twenty lines in a second (**held**) | all twenty | all twenty | all twenty |

Second Life's cut looks like what fits one packet: the name and the text
share it. A viewer need not care which rule applies — the reference never
sends more than 1023 bytes — but it must show what arrives, cut or not.

## Typing

A typing notice is a line of the type `start typing` or `stop typing`.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| who is sent a start and a stop, standing together (**held**) | the other avatar and the typist itself | the same | the same |
| a stop with no start before it (**held**) | passed on | passed on | passed on |
| a start that carries text (**held**) | passed on as a typing notice | the same | the same |
| a start on channel 5 (**held**) | nobody is sent it | nobody is sent it | nobody is sent it |
| how far (**held**) | **fully audible** within a say's reach (12.6 m, 18.7 m); **barely audible** past it and within a shout's (36.5 m); not sent beyond (121.5 m) | the whole region, fully audible: at 126 m as at 2 m | as the flavour's grid |

The reference viewer shows nothing of a notice that is not fully audible,
and neither does ours now: a resident sixty metres off is not "typing" at
anybody. A stop is taken however it is marked, so a typist who walks out of
earshot in the middle of a line does not stay typing.

## A script's lines

The probe prim stands beside B; its owner A is the one that moves away. Not
asked of the fake grid, which runs no scripts.

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| `llWhisper`, `llSay`, `llShout` on channel 0 (**held**) | the three types, the three ranges an avatar's lines have: whisper unheard at 12.6 m, say heard at 18.7 m and not at 36.5 m, shout heard at 36.5 m and not at 121.5 m | the same types and ranges — but **`llShout` names nobody as the owner** |
| `llRegionSay` on channel 0, and on channel 5 (**held**) | no avatar is sent either | the same |
| `llOwnerSay` (**held**) | the owner alone, anywhere in the region (121.5 m); type `owner`, the owner as owner, said at the prim | the owner alone, anywhere (126 m); type `owner` — but the **prim itself as owner** and **the middle of the region** (128, 128, 30) as where it was said |
| `llRegionSayTo` an avatar on channel 0 (**held**) | that avatar alone, anywhere in the region; type `direct` | the same |
| `llRegionSayTo` an avatar on channel 5, and on the debug channel (**held**) | nothing | nothing |
| `llWhisper`, `llSay`, `llShout`, `llRegionSay` on the debug channel (**held**) | a line of the type `debug` to **everybody in reach, the owner or not**; the reach is one for all four — heard at 18.7 m, unheard at 36.5 m | a `debug` line to the whole region (126 m); `llShout` and `llRegionSay` name nobody as the owner |
| `llSay(0, "")` (**held**) | an empty line | an empty line |
| a line of 1500 bytes (**held**) | the first 1024 | the first 1000 |
| a prim with no name (**held**) | an empty name | an empty name |

OpenSim's odd `llOwnerSay` is its broadcast path (`ChatModule.OnChatBroadcast`),
which states the source as the owner and the region's middle as the place.
A viewer that draws a beam from a speaking object, or keys a mute on the
owner, gets neither right there.

## Across a border

Both grids carry a line over a region border, by the same ranges, measured
to where the hearer stands in the next region.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a say 8–12 m away over the border, a shout at 36–40 m (**held**, each way) | heard | heard | heard |
| a whisper at 7.5–12.4 m (**held** as "heard just when within 10 m") | as within a region | as within a region | as within a region |
| a say at 36–40 m, a shout at 117–122 m (**held**) | unheard | unheard | unheard |
| where the line says it was said (**held**) | in the **hearer's** region's coordinates — a point outside that region | in the **speaker's** region's coordinates | as the flavour's grid |
| which circuit carries it | not told apart by a client; the coordinates are the hearer's region's | the **child circuit** the hearer's viewer holds to the speaker's region | Second Life's flavour says it from the hearer's region, OpenSim's down the child circuit |
| typing, within a say's reach (**held**) | fully audible | fully audible | as the flavour's grid |
| typing at 36–40 m | mostly nothing: of the four times it was asked in two runs it came once, barely audible | fully audible (**held**) | Second Life's flavour sends none |
| typing at 117–122 m (**held**) | nothing | fully audible | as the flavour's grid |

OpenSim's neighbour measures to where it was **last told** the hearer
stands, and a region tells its neighbours of an agent only once it has moved
ten metres (`CHILDUPDATES_MOVEMENT`) or changed its draw distance by more
than 32 m. Near a range's edge the answer can therefore be up to ten metres
out: a whisper 8 m over the border went unheard one way and was heard the
other until the case had each viewer restate its draw distance first.

Our client used to read local chat on the root circuit alone, so on OpenSim
a neighbour's line never reached the viewer: the first OpenSim run counted
nine lines dropped on a child circuit. `sl-proto` now reads
`ChatFromSimulator` on any circuit. A line's position is in the coordinates
of the region that sent it, and on OpenSim that is not the region the
listener stands in.

## What the viewer does with it

- A line or a start of typing that is not fully audible is not shown
  (`sl-viewer-chat`, `sl-viewer-people`, the name tag's typing mark).
- A line heard over a border is shown like any other.
- A line typed on the debug channel is shown as the kind its grid made of
  it — a say on Second Life, a debug line on OpenSim (`e2e_pilot`, on a fake
  grid of each flavour).

## Not measured

- A script's run-time error on the debug channel
  (`gridspec-lsl-runtime-errors`), a line from an attachment or a child
  prim, `llInstantMessage`, and what a script's listen hears
  (`gridspec-lsl-comms`).
- Where exactly between 18.7 m and 36.5 m a debug line stops on Second Life.
  The fake grid takes it to be a say's 20 m.
- A parcel that hides its avatars (`SeeAVs` off), which OpenSim's source
  keeps their chat inside of, and a muted speaker (`gridspec-mute-list`).
- Which circuit Second Life sends a neighbour's line down.
