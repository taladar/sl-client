# Local chat and instant messages

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
  prim, and what a script's listen hears (`gridspec-lsl-comms`).
- Where exactly between 18.7 m and 36.5 m a debug line stops on Second Life.
  The fake grid takes it to be a say's 20 m.
- A parcel that hides its avatars (`SeeAVs` off), which OpenSim's source
  keeps their chat inside of, and a muted speaker (`gridspec-mute-list`).
- Which circuit Second Life sends a neighbour's line down.

## Instant messages

A viewer's `ImprovedInstantMessage` and a script's `llInstantMessage` reach
their avatar as the same message: a dialog, who it is from and under what
name, an id, a region and a place, whether it was kept while the avatar was
away, a time, an estate, a bucket and a text.

Measured on 2026-10-13 by the conformance case `instant-messages`: three
runs on aditi (Mauve) and four on the local OpenSim, the last of each held
to 95 answers. Avatar **A** writes and owns a probe prim; avatar **B** is
written to, mutes A for a while, and logs out three times to be written to
while away. No answer changed between two runs. The fake grid passes no
instant message on yet (`server-fake-grid-im-relay`), so the table has no
column for it.

### What a message says of itself

A viewer's ordinary message (dialog 0) as the other avatar is sent it. The
viewer wrote the session id of the two avatars (their ids exclusive-ored)
as the id, its avatar's name, and nothing else.

| field | Second Life | OpenSim |
| --- | --- | --- |
| from, name, to, id, text (**held**) | as sent | as sent |
| region (**held**) | an id | an id |
| place (**held**) | where the sender stands | zero |
| kept flag (**held**) | not kept | not kept |
| time (**held**) | none | the time it was sent |
| estate (**held**) | 1 | 1 |
| bucket (**held**) | one zero byte | empty |

What a viewer may claim (**held**): a message sent with a made-up name, id,
region, place, time, estate and bucket.

| | Second Life | OpenSim |
| --- | --- | --- |
| name | **rewritten** to the sender's | as sent |
| id | **rewritten** to the session id of the two | as sent |
| region, estate | rewritten to the sender's | rewritten to the sender's |
| place | rewritten to where the sender stands | as sent |
| time | **as sent** — the one thing Second Life takes a viewer's word for | rewritten to now |
| bucket | rewritten to one zero byte | as sent |
| a message flagged as kept | passed on at once, not flagged | the same |

A viewer on OpenSim can therefore sign a message with any name: what says
who wrote it is the agent id alone, on both grids.

### What is passed on

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| the sender is told (**held**) | nothing | nothing |
| a message without text, with a line break, a tab and spaces at the end (**held**) | as sent | as sent |
| a long text (**held**) | the first **1023 bytes**: 1023 arrive whole, 1024, 1100, 2000 and 3000 are cut | the same |
| typing start and stop (**held**) | passed on, id the session id; the typist is told nothing | the same |
| an automatic reply, dialog 20 (**held**) | passed on as dialog 20 — from **no region, place zero, estate 0** | passed on as dialog 20, stated like an ordinary message |
| a message to oneself (**held**) | arrives, its id the avatar's own (an id exclusive-ored with itself is nil; both grids state the avatar instead) | the same |
| a message to an id that is nobody's (**held**) | "User not online - message will be stored and delivered later." | "User is not logged in. Message saved." |
| a message to no id (**held**) | nothing | nothing |
| dialog 1 (message box) and a dialog that is none (60) from a viewer (**held**) | dropped | dropped |
| dialog 19 (a script's message) from a viewer (**held**) | **dropped** | **passed on** as dialog 19 from the avatar |
| twenty messages in a second | **fourteen** arrive, in order; the sender is then told, by an ordinary message from no id signed "Second Life": "You have sent out excessive instant messages, and your ability to send IMs has been temporarily suspended. Try again later." Its next message, most of a minute later, was passed on (the notice **held**, the count not) | all twenty, in order (**held**) |

### A script's message

`llInstantMessage` from a prim A owns, to B. Dialog 19.

| field | Second Life | OpenSim |
| --- | --- | --- |
| from (**held**) | the **owner's** agent id | the same |
| name (**held**) | the prim's name | the same |
| id (**held**) | the prim | the same |
| region (**held**) | an id | an id |
| place (**held**) | zero | where the prim stands |
| time (**held**) | none | the time it was sent |
| estate (**held**) | 1 | **101** (the viewer's message there says 1) |
| bucket (**held**) | `Region/x/y/z`, where the prim stands | the same |
| a text of 1500 bytes (**held**) | the first 1023 | the first 1023 |
| a message without text (**held**) | arrives, empty | arrives, empty |
| to its owner (**held**) | arrives, the same shape | the same |
| to an id that is nobody's (**held**) | the owner is told nothing | the same |

### A mute

B mutes A (`UpdateMuteListEntry`, by agent), and its mute list then names A
on both grids.

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| A's message to B (**held**) | **passed on** | **passed on** |
| A's typing (**held**) | passed on | passed on |
| the message of a prim A owns (**held**) | passed on | passed on |
| B's message to the avatar it muted (**held**) | arrives | arrives |
| A is told it is muted (**held**) | no | no |

Neither grid keeps an instant message from an avatar that muted its sender:
the mute list is storage, and the filter is the viewer's. Ours drops a muted
resident's messages and typing notices (`sl-viewer-people`), as the
reference does.

### To an avatar that is away

B logs out; A sends two messages, a typing notice, an automatic reply, a
message of 2000 bytes and a script's message.

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| A is told, per ordinary message (**held**) | "User not online - message will be stored and delivered later." | "User is not logged in. Message saved." |
| … as (**held**) | an ordinary message **from B's id**, signed "Second Life", id the session id, no region, place zero, no time, estate 0, a bucket of one zero byte | the same but signed "System", with the time and an empty bucket |
| typing (**held**) | told nothing, nothing kept | the same |
| an automatic reply (**held**) | told nothing; **kept** | **refused**: "Unable to send instant message. User is not logged in.", as dialog 20 from A's own id signed "System"; nothing kept |
| a script's message (**held**) | the owner is told nothing; kept | the same |

B logs in again.

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| sent without asking, in 25 s (**held**) | nothing | nothing |
| `RetrieveInstantMessages` (**held**) | everything kept, in the order sent | everything kept, in the order sent |
| the `ReadOfflineMsgs` capability (**held**) | 200, everything kept, in the order sent, on the first reading | no such capability |
| asked again, by either road, and by the other road (**held**) | nothing: whichever road was asked first emptied the store for both | nothing |

A kept message over `RetrieveInstantMessages` (**held**) is the message as
it would have arrived, with these differences:

| field | Second Life | OpenSim |
| --- | --- | --- |
| kept flag | kept | kept |
| time | the time it was **sent** (a message that arrives at once states none) | the time it was sent |
| the kept automatic reply | from no region, place zero, estate 0, as it arrives live | — |
| a text of 2000 bytes | the first 1023 | the first 1023 |

The capability answers **an array holding one array of records** — the one
array form the reference reads (`*contents.beginArray()`). A record
(**held**):

| key | value |
| --- | --- |
| `from_agent_id`, `from_agent_name`, `to_agent_id`, `message`, `dialog`, `from_group` (a boolean) | as over UDP |
| `transaction-id` | **nil, in every record** |
| `asset_id` | the id the message carries over UDP: the session id of a typed message or an automatic reply, the prim of a script's message |
| `offline` | 1 |
| `timestamp` | the time it was sent |
| `message_time` | the same as a date and time text |
| `region_id`, `position` (an array of three), `parent_estate_id`, `estate_id` | the sender's region, place and estate (1) — for the automatic reply too, which over UDP states none of them |
| `binary_bucket` | as over UDP |

So Second Life's in-world read-back, recorded on 2026-08-12 as "stays
empty", was never empty: our decoder read the outer array's one element as
a record and found none. `sl-proto` now reads the nested array, and takes
`asset_id` for the id of a typed message, an automatic reply and a script's
message where `transaction-id` is nil, so that both roads hand a client the
same message. An offer's or an invitation's id is left nil, which is what
tells a viewer to answer it over the offline capabilities ([Friends and
presence](friends.md)).

### What the viewer does with it

- **The grid's notice is nobody's line.** Both grids sign "your message was
  kept" with the id of the resident who is away and a name of their own
  ("Second Life", "System"), from no region and no place. Our viewer used to
  file that under the resident's conversation *as the resident* and take
  the signature for their legacy name. It now shows the line in that
  conversation under the grid's signature, linked to nobody, and learns no
  name from it; a notice from no id (Second Life's suspension notice) goes
  with the nearby chat instead of opening a conversation with nobody; and
  the automatic reply does not answer either (`sl-viewer-people`, unit
  tests).
- **The drain stays on `RetrieveInstantMessages`.** It brings everything on
  both grids, with every id, and empties the capability's store as well, so
  nothing is shown twice. The capability brings nothing more, and brings an
  offer without the id needed to answer it over UDP.
- **Neither grid replays anything unasked**, so there is nothing to
  de-duplicate.

### Not measured

- An inventory offer kept for an avatar that is away, over either road
  (`gridspec-inventory-offers`). A friendship offer is measured in [Friends
  and presence](friends.md), a group invitation in [Groups](groups.md).
- How long Second Life's suspension after a burst lasts, and the rate that
  triggers it.
- How many messages a grid keeps for one avatar, and for how long.
- Messages forwarded as mail: both test accounts have it off.
- A message between avatars in different regions; a session or a
  conference (`gridspec-group-chat-and-conference`).
- A mute by name or of an object (`gridspec-mute-list`).
