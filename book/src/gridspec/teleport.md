# Teleport

How each grid carries out a teleport the client asks for, and how it refuses
one; how it carries the offers and requests avatars make each other
([Offers and requests](#offers-and-requests)); and which neighbouring regions
it holds an agent in and how it hands the agent to one when it walks over a
border ([Neighbours and crossings](#neighbours-and-crossings)). The messages
and the client's handover are described in
[Teleport](../content/teleport.md).

Measured on 2026-10-07 on Second Life's beta grid (aditi) and the local
OpenSim standalone (a 2×2 block of regions), by:

- scripted `sl-repl` runs with the trace log on, which is where the raw
  messages, their transport and the fields no event carries (`LocationID`, the
  `TeleportFinish` body) were read — three on aditi, from the sandbox, a
  region with five neighbours and a mainland region, and three on OpenSim;
- `teleport-local-phases`, a teleport within the agent's region;
- `teleport-cross-region`, a teleport to a neighbouring region;
- `teleport-failed`, a teleport to a region that does not exist and one to a
  landmark the grid does not hold;
- `teleport-cancel`, a `TeleportCancel` sent on the heels of the request;
- `teleport-access-refused`, a teleport into a region rated above the agent's
  maturity preference (aditi and both fake flavours).

The five cases hold aditi, OpenSim and both fake flavours to these answers,
except where a row says the fake grid does not model it.

## Within a region

A request naming the agent's own region is answered with a `TeleportStart`
and a `TeleportLocal` on both grids, a fifth of a second apart on aditi and a
millisecond apart on OpenSim, with no progress line between them.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the flags of the `TeleportStart` and the `TeleportLocal` | `VIA_LOCATION` and `WITHIN_REGION` (`0x20010`) | `VIA_LOCATION` alone (`0x10`) | each flavour's |
| the `TeleportLocal`'s `LocationID` | 2 | 2 | 2 |
| the look-at the `TeleportLocal` states | the unit vector from the landing position towards the region's origin, whatever was asked for: five teleports, five look-ats equal to the negated, normalised position (asked to face east at 128/128/5000, told `-0.025 -0.026 -0.999`) | the direction asked for, flattened to the horizontal (asked north, told north); east when nothing horizontal is left (source) | each flavour's |
| a height far above or below the ground | echoed as asked: `z` 5000 and `z` −50 both came back in the `TeleportLocal` | 5000 m accepted; below the ground is lifted onto it (source) | lifted onto the ground when below it, on both flavours |
| a position past the region's edge (`x` 300 under the agent's own handle) | a `TeleportLocal` stating `x` 300; no crossing followed in fifteen seconds | carried out as a teleport into the neighbour that position lies in: a `TeleportFinish` for the region to the east, arrival at `x` 44 | not modelled: the position is taken as given, in the agent's region |
| a parcel that routes arrivals | the `TeleportLocal` states where the agent was put, not where it asked to go (asked for 120/136 in the sandbox region, landed at 91/200) | lands where asked | lands where asked |

A viewer applies the stated look-at to the avatar at once, as the reference
does (`process_teleport_local`), so on Second Life a local teleport leaves
the avatar facing the region's south-west corner. That is the grid's answer
and both viewers show it.

## To another region

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the messages, in order | `TeleportStart`, `TeleportProgress` `resolving`, `TeleportProgress` `Sending to destination.`, then `TeleportFinish` over the event queue | `TeleportStart`, then `TeleportFinish` over the event queue | each flavour's |
| the progress lines | a key and then a sentence that is no key, 0.11 s apart | none at all | each flavour's |
| a home teleport's lines | `sending_home`, `resolving`, `Sending to destination.` | not measured: the test avatar has no home set, and the request is refused | Second Life's on that flavour, none on OpenSim's |
| a landmark teleport's lines | `sending_landmark` first (seen ahead of a refusal; no landmark was at hand for a successful one) | none ahead of a refusal | `sending_landmark`, then the two lines, on the Second Life flavour |
| the flags of the start, the lines and the finish | the kind of teleport and nothing else: `VIA_LOCATION` (`0x10`), `VIA_HOME` (`0x20`), `VIA_LANDMARK` (`0x08`), `VIA_LURE` (`0x04`) | the start says the kind; the **finish says `VIA_LOCATION` whatever the kind** (an accepted lure started `0x04` and finished `0x10`), keeping only the flying bit of the request (source: `EventQueueGetHandlers.TeleportFinishEvent`) | each flavour's |
| the `TeleportFinish` body | `AgentID`, `LocationID` 3, `RegionHandle`, `SeedCapability`, `SimAccess`, `SimIP`, `SimPort`, `TeleportFlags`; no region size | the same with `LocationID` 4, and `RegionSizeX` / `RegionSizeY` | each flavour's |
| anything naming the destination before the finish | nothing | nothing | nothing |
| request to arrival | 0.5 s between regions of one simulator host, 2 s to another host | 0.1 s over loopback | milliseconds |
| where the agent lands | where the region routes arrivals: a teleport to 128/128 in a region with a landing point arrived at 8/10, one into the sandbox at its telehub | where it asked | where it asked |
| the source circuit afterwards | no `DisableSimulator` within 25 s of a teleport to a neighbour; not established for a distant one (the client has dropped the circuit by then and no longer listens to it) | none after a teleport to a neighbour, which is every region of the local grid; a distant one cannot be measured there | a `DisableSimulator` for a distant source, on both flavours |

`Sending to destination.` is the text the reference viewer shows for the key
`sending_dest`; Second Life sends the text. A client that resolves progress
keys to its own strings has to pass a line it does not know through.

One thing seen on the way and not explained: the sandbox region's
`RegionHandshake` rated it Moderate (`SimAccess` 21) at login and Adult (42)
on arriving there by teleport, in the same session; the world map and the
`TeleportFinish` both said 42.

## Refusals

The two grids refuse in opposite orders and over different transports.
Second Life starts the teleport, narrates as far as it got, and then fails it
with a `TeleportFailed` **event on the event queue** —
`{ Info: [ { AgentID, Reason } ], AlertInfo: [ { Message, ExtraParams } ] }`
— about a tenth of a second after the last line. OpenSim sends the UDP
`TeleportFailed` *instead of* a `TeleportStart`, with no `AlertInfo` block.

| the request | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a region handle no region answers to | start (`0x10`), `resolving`, then `Reason` `no_host` with alert `no_host` | `The region you tried to teleport to was not found`, followed by a `MapBlockReply` marking that map cell empty (access 254) | each flavour's refusal; the map block is not sent |
| a landmark asset the grid does not hold | start (`0x08`), `sending_landmark`, then `nolandmark_tport` with alert `nolandmark_tport` | `Could not find the landmark asset data` | each flavour's |
| home, with no home set | not measurable: every test avatar has one | `Home set not` | OpenSim's on that flavour; `invalid_tport` on the Second Life one, unmeasured |
| a region rated above the agent's maturity preference | start, both lines, then a paragraph beginning `You aren't allowed in that Region due to your maturity Rating.` with alert `RegionTPAccessBlocked`, its `ExtraParams` an LLSD/XML map `{ _region_access: <SimAccess> }` | no check: a region set Moderate for the probe admitted an agent whose preference was General | each flavour's |
| home, into a region rated above the preference | carried out: an agent whose stored preference was General was sent home to an Adult region | — | carried out |
| a region at its agent limit | not measured: needs estate rights | `The region is full` (the limit set to 0 on the console for one probe) | not modelled |

The fake grid holds an event-queue failure behind the client's
acknowledgement of the last UDP line it sent. The live grid's tenth of a
second is what orders the two transports there; without it a failure can
overtake its own `TeleportStart`, which a client reads as a second teleport
beginning.

When OpenSim's region was set Moderate, its handshake said so (`SimAccess`
21) while the world map and the `TeleportFinish` went on saying General (13)
until the grid service's record was refreshed.

Refusals not provoked here: an estate ban, a full region on Second Life, and
a destination that never confirms the arrival (OpenSim's source words that as
`Problems connecting to destination …`; the fake grid sends `timeout_tport`
by each flavour's transport). What becomes of a lure that cannot be honoured
is under [Offers and requests](#offers-and-requests).

One refusal was provoked by accident and is OpenSim's alone. A teleport asked
for two milliseconds after arriving in a region — a conformance case accepting
a lure back the moment it had stepped next door — hung, and twenty-five
seconds later the region *first* left sent a `TeleportFailed` for the *first*
teleport (`Problems connecting to destination …`) over what was by then a
child circuit. That region had still been waiting to see the agent settle
where it sent it, found it a child there again, and gave the transfer up
(`UpdateAgent failed on teleport … Keeping avatar`), taking the second
teleport down with it in an exception. Ten seconds between the two is enough;
nobody presses a button that fast, and the client ignores a failure that
arrives on a child circuit.

## Cancelling

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a `TeleportCancel` straight after the request | the teleport is abandoned: a `TeleportFailed` over the event queue, `Reason` `Teleport cancelled.`, alert `TPCancelled`. In one run the `TeleportStart` and `resolving` still arrived ahead of it, in another nothing did | the teleport goes through and the agent arrives | each flavour's outcome; the Second Life flavour always sends the start and its lines first |
| a `TeleportCancel` after the arrival | nothing | not measured | nothing |

OpenSim's source honours a cancel only between creating the agent at the
destination and sending the `TeleportFinish` — tens of milliseconds over
loopback — and then abandons the teleport without telling the client. A
cancel sent with the request arrives before that window opens and is lost.
The fake grid's OpenSim flavour takes the measured outcome and never honours
one.

So a client cannot treat its own cancel as the end of the teleport. The
session returns to its active state when it sends one, and takes up the
teleport again if the grid carries on: a `TeleportStart` or `TeleportFinish`
that follows is a teleport the grid decided on.

## Offers and requests

One avatar offers another a teleport with a `StartLure`; the grid delivers it
as an `ImprovedInstantMessage` of dialog `IM_LURE_USER` (22) whose id is the
*lure id*, and the other accepts by quoting that id in a
`TeleportLureRequest`, or declines with an instant message of dialog
`IM_LURE_DECLINED` (24) carrying it. A *request* for an offer is an instant
message of dialog `IM_TELEPORT_REQUEST` (26). The teleport an acceptance
starts is an ordinary one, flagged `VIA_LURE`.

Measured on 2026-10-07 with two avatars on each grid, by:

- `teleport-offer-accept`, an offer accepted from within the offerer's region
  and another from the region next door, watching the offerer for ten seconds
  after each;
- `teleport-offer-decline`, an offer declined and then accepted anyway, and a
  second offer accepted twice;
- `teleport-request`, a request, and the offer that answers it;
- `teleport-lure-unknown`, an acceptance of a lure nobody offered (one avatar;
  also held on both fake flavours);
- `teleport-lure-offline`, a lure accepted ten seconds after its offerer
  logged out, and an offer made to an avatar that is logged out;
- `teleport-lure-rated`, a lure into an Adult region accepted by an avatar
  whose maturity preference is General (aditi only).

### The offer as delivered

| field | Second Life | OpenSim |
| --- | --- | --- |
| the lure id | opaque, and new for every offer | the **place**: the offerer's region handle and position packed into the id (`Util.BuildFakeParcelID`; `x` and `y` truncated to the metre, `z` truncated and raised by two). Two offers from one spot carry one id |
| the binary bucket | text, 35 bytes with a padding space and a terminator: `255232\|256512\|10\|10\|42\|-1\|0\|-0\|PG` — the destination region's corner in global metres, the landing position and a facing in whole metres, and the region's rating (`PG` and `A` seen; the reference viewer also reads `M`) | empty |
| the message | as sent | as sent |
| the sender's name and id | the offerer's | the offerer's |
| `RegionID` | the offerer's region | the offerer's region |
| `Position` | zero | the offerer's position |
| `ParentEstateID` | 1 | 1 |
| `Timestamp` | none | set |
| `Offline` | no | no |
| anything sent to the offerer | nothing | nothing |

So a Second Life offer says where it leads and how that place is rated before
anything is accepted, and an OpenSim offer says where — in the id — and
nothing of a rating. The landing position Second Life states is the one the
accepter is put at: an offerer standing at a parcel's landing point at
10/10/42 sent `10|10|42`, and both acceptances landed there.

OpenSim's source sends an administrator's offer as dialog 25
(`IM_GODLIKE_LURE_USER`) instead; neither grid's godlike lure was provoked.

### Accepting

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| from within the offerer's region | `TeleportStart`, `TeleportProgress` `completing`, `TeleportLocal`, every one flagged `VIA_LURE` and `WITHIN_REGION` (`0x20004`); 0.18 s | `TeleportStart`, `TeleportLocal`, flagged `VIA_LURE` (`0x04`); a millisecond | each flavour's |
| from another region | `TeleportStart`, then `completing`, `resolving`, `Sending to destination.`, then the `TeleportFinish`; every one flagged `VIA_LURE`; 0.54 s from a neighbour | `TeleportStart` (`VIA_LURE`), then the `TeleportFinish` (`VIA_LOCATION`), no line; 0.07 s | each flavour's |
| where the accepter lands | at the position the offer's bucket states | at the position the lure id packs | at the position a lure id packs; an opaque id is read as its offerer's agent id and lands at the centre of the region they are in |
| which way it faces | towards the region's origin, as after any local teleport there | east | each flavour's |
| what the offerer is told | nothing: no `IM_LURE_ACCEPTED` (23), no alert, in ten seconds | nothing | nothing |

`completing` is the first line of a lure and not the last, whatever its name
says; the reference viewer's text for it is *Completing teleport.* A client
that took it for the end of the teleport would be wrong by the whole
teleport.

### Declining, and how long a lure lasts

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| whether the decline reaches the offerer | no: nothing arrives in ten seconds | no: only five dialogs of instant message are relayed, and this is not one (source: `InstantMessageModule.OnInstantMessage`) | not relayed |
| accepting a lure after declining it | **no answer at all** — no start, no failure, no alert; the client's own thirty-second deadline ends it | carried out: the lure is a place and cannot be withdrawn | each flavour's for a lure it cannot resolve; neither holds lures to spend |
| accepting a lure a second time | no answer at all: a lure is spent by its first use | carried out, every time | as above |
| accepting a lure nobody offered | no answer at all | read as a place all the same: `TeleportFailed` over UDP, `The region you tried to teleport to was not found`, sent instead of a start | each flavour's |
| accepting a lure ten seconds after its offerer logged out | carried out, landing where the offerer stood | carried out | carried out for a place; an opaque id whose offerer is gone gets the flavour's answer to a lure nobody offered |
| accepting a lure into a region rated above the accepter's maturity preference | refused with a `TeleportFailed` **alone** — no start and no line, unlike a location teleport to the same region — the same sentence and the alert `RegionTPAccessBlocked` with `_region_access` 42; the accepter stays where it was | not provoked (the local regions are all General); the source sends a lure down the path a location teleport takes, which has no such check | each flavour's |
| an offer to an avatar that is logged out | the offerer is told nothing, and the offer was not there when the avatar logged in twenty seconds of watching later, nor after asking for stored messages | the offerer is told nothing and the offer is not stored (the offline-message module keeps five dialogs, and a lure is not one) | not relayed |

Second Life answering a dead lure with *nothing* is why the client has to
keep a deadline of its own: a viewer whose Teleport button is pressed on an
offer already used, declined in another session, or simply made up waits on a
teleport the grid will never mention again. The session fails it after thirty
seconds and records that the failure was its own (below).

Not measured: how long an unused lure lasts on Second Life, whether a lure
refused for its rating is spent, an offer naming several avatars at once, and
Second Life's side of an offer to somebody offline beyond what the harness
can see — it could not read a plain stored instant message back there either
(`offline-msg-fetch`).

### Requests

| field | Second Life | OpenSim |
| --- | --- | --- |
| delivered to the avatar asked | yes | yes (source: `LureModule` relays dialog 26 itself) |
| the id | nil, as sent | the exclusive-or of the two agent ids — the id of their one-to-one conversation. OpenSim gives every instant message sent without an id that one (`LLClientView.SendInstantMessage`) |
| the binary bucket | one zero byte | empty |
| `RegionID` | none | the requester's region |
| `ParentEstateID` | 0 | 1 |
| `Timestamp` | none | set |
| `Position` | as sent | as sent |
| anything sent to the requester while unanswered | nothing | nothing |

There is no accept or decline for a request, on either grid. A viewer says
yes by sending an ordinary offer back, which arrives as any offer does, and
no by sending nothing; the reference's No button is silent.

The fake grid relays no instant message between its sessions yet, so offers,
declines and requests reach nobody there; a test plays the other avatar
through the session's own grid-side handle (the roadmap's
`server-fake-grid-im-relay`).

## Neighbours and crossings

A simulator holds a **child agent** for an avatar in each neighbouring region
the avatar can see into: it announces the region with an `EnableSimulator`,
hands over that region's seed capability with an
`EstablishAgentCommunication`, the client opens a child circuit there, and
the region streams its scene down it. Walking over the border then promotes a
circuit the client already holds: a `CrossedRegion` names it, the client
sends its `CompleteAgentMovement` there, and nothing is torn down.

Measured on 2026-10-07 on aditi — an avatar standing in the north-eastern
region of a two-by-two block, eight to ten metres from its western and
southern borders — and on the local OpenSim's two-by-two block, by:

- scripted `sl-repl` runs with the trace log on (five on aditi, two on
  OpenSim), which is where the transports, the event bodies and the messages
  no event carries were read;
- `neighbour-child-circuits`, the neighbours as an arrival is told of them;
- `draw-distance`, the draw distance stepped to 100 m, 32 m and 512 m;
- `region-crossing`, a walk over the nearest border a neighbour shares and
  back again — on foot on OpenSim, by flight on aditi (below).

The three cases hold aditi, OpenSim and both fake flavours to these answers,
except where a row says the fake grid does not model it.

### Which neighbours, and when

Both grids announce over the **event queue** and retire with a
`DisableSimulator` over UDP, down the child circuit being retired. Neither
sent a UDP `EnableSimulator`.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the regions announced to an arrival | all three others of the block, the one touching at a corner included | the same | every region one slot away, as each flavour's draw-distance rule allows |
| how they are announced | every `EnableSimulator` in one event-queue batch, 1.4 s after the handshake; each `EstablishAgentCommunication` only once the client has opened that circuit, 0.2–0.6 s later | an `EnableSimulator` and its `EstablishAgentCommunication` together, one neighbour every half second (source: a `Thread.Sleep(500)` in `EnableChildAgents`), the first 0.9 s after the handshake | the pair together, with nothing between neighbours, on both flavours |
| the `EnableSimulator` body | `Handle`, `IP`, `Port` | the same and `RegionSizeX` / `RegionSizeY` | each flavour's |
| the `RegionHandshake` a child circuit is greeted with | sent twice | sent once | each flavour's |
| an announcement repeated | the `EnableSimulator` of each neighbour sharing an edge again every 60 s; the `EstablishAgentCommunication` of the one touching at a corner again every 5 s, for as long as it is held; all of them again when the draw distance changes. Always the same seed. (A mainland region with five neighbours repeated *both* events for every neighbour about every five seconds: [Region arrival](region-arrival.md)) | on one login of four — two seconds after the avatar's previous logout — every neighbour was announced a second time, two seconds after the first | not modelled |
| what a child circuit carries | the region's ground, parcel overlay, objects and kills, its coarse locations, and the avatars standing there: their object updates, animations, appearance, attached sounds and `ViewerEffect`s. No `SimStats` and no time messages | ground, parcel overlay, objects, coarse locations. No avatars were there to see | the region's ground, objects and a marker |
| a draw distance that reaches a neighbour | 128 m for one sharing an edge (126 m did not reach, 128 m did); 128·√2 = 181 m for the one touching at a corner (180 m did not, 184 m did). Both edges, 8 m and 10 m away, answered at the same figure, so it is no distance to anything | the draw distance plus 64 m, held between 96 m and 255 m, as the half-width of a square around the *avatar*: a region that square touches is held (source: `ScenePresence.RegionViewDistance`, `EntityTransferModule.RegionsInView`). From the middle of a region 32 m reached nothing and 100 m everything; twelve metres from a border 32 m reached across it | each flavour's rule |
| a neighbour the draw distance stops reaching | retired **50 s** later (50.3 s to 51.0 s over four runs), all three within 40 ms of each other | retired within a second (0.5 s to 0.8 s) | each flavour's delay |
| a neighbour the draw distance reaches again | announced within 1.3 s, with a new seed | announced within a second, half a second apart, with a new seed | announced at once, with a new seed |
| at logout | nothing down the child circuits ([Logout](session.md#logout)) | a `DisableSimulator` down each | each flavour's |

The figure Second Life compares the draw distance with is half the distance
between the two regions' centres. It was measured from one spot only; that a
neighbour eight metres away needed the same 128 m as one ten metres away is
what says the avatar's position is not in it.

One thing seen once and OpenSim's alone: a login two seconds after the same
avatar's logout found the region touching at a corner still holding the old
session's child agent (`Reusing existing child scene presence`). It was
announced, never said a word down the new child circuit, and the client
dropped the circuit as silent 45 s later.

### Walking over a border

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the message | `CrossedRegion` over the event queue | the same | the same |
| its body | `AgentData` (`AgentID`, `SessionID`), `Info` (`LookAt`, `Position`), `RegionData` (`RegionHandle`, `SeedCapability`, `SimIP`, `SimPort`) | the same and `RegionSizeX` / `RegionSizeY` in `RegionData` | each flavour's |
| `Info.Position` | in the destination region, two metres past the border (254.2, 254.1 and 254.0 flying west and south; 1.9 and 2.0 flying back) | in the destination region, half a metre past it (0.53 walking east, 255.4 back) | where the test puts the agent |
| `Info.LookAt` | the unit direction of travel: `-1 0 0` flying west, `0 1 0` flying north | zero at a walk. The source passes the agent's horizontal velocity there when it crosses flying (`EntityTransferModule.CrossAgentIntoNewRegionMain`) | a facing on the Second Life flavour, the velocity the test names on the OpenSim one |
| the seed it names | the one that neighbour's `EstablishAgentCommunication` gave; on the way back, the one the login gave | the same | the child session's |
| sent ahead of it | nothing that belongs to it | an `AgentDataUpdate` from the destination, 85 ms ahead | nothing |
| what the destination says once the client has completed its movement | its `RegionHandshake` a third time, then `AgentMovementComplete` and `HealthMessage`, 0.18 s after the `CrossedRegion`; then an `AgentStateUpdate` and the agent's `ParcelProperties` over the event queue | `AgentMovementComplete` within a millisecond, the agent's `ParcelProperties`, and no handshake | each flavour's |
| the agent's own avatar | re-sent by the destination under a new local id 1.2 s later, after a `KillObject` for the id before it. The region left sends no kill for its copy | re-sent by the destination under a new local id 5 ms later, with its `AvatarAppearance`. No kill from either region | re-sent by the destination; no kill |
| the region left behind | stays a child: its handshake comes a fourth time 1.4 s later, and it goes on streaming | stays a child and goes on streaming | stays a child |
| the other neighbours | each one the new region also borders is announced again by it, 1.2 s to 1.4 s after the arrival, and greets its circuit again. Walking back, nothing was announced | nothing announced, nothing retired: every region of the block borders every other | a child no longer bordering the new region is retired, on both flavours |
| a neighbour retired after a crossing | none in the 25 s watched; the block is too small for a crossing to leave one behind | none | as above, unmeasured |
| request to arrival | 0.2 s | 0.1 s over loopback | milliseconds |
| a teleport event on the way | none | none | none |

Neither grid tells the crossing agent's own viewer that its avatar has left
the region it walked out of; the client is left holding two copies of one
avatar, and drops the old region's the moment the new region's arrives.

The aditi avatar could not be *walked* to its border. Its region routes every
arrival to a landing point, and an avatar standing there is held by it: ninety
seconds of the forward key moved it two centimetres, while it turned to face
where it was told. A flight from the same spot crossed the border eight
metres away in four seconds, so the case flies on Second Life and walks on
OpenSim.

Not measured: a **seated** crossing — a vehicle carrying its riders over a
border. Neither grid has a vehicle to hand: it needs a script uploaded into a
prim and an avatar seated on it, and aditi has yet to answer a sit request
from this client at all (the roadmap's `gridspec-sit-stand`). Also not
measured: a crossing that leaves a neighbour out of view (both blocks are two
by two), a crossing at a corner, and what either grid does when the client
never completes its movement.

## What the client does with it

- **Both failure transports end the teleport.** The event-queue
  `TeleportFailed` was not handled before this was measured: a refused
  teleport on Second Life ran into the session's own thirty-second timeout.
- `Event::TeleportStarted` and `Event::TeleportLocal` carry their flags.
- The viewer's teleport display resolves what the grid sent. A failure named
  by key — as the reason, or in the alert beside a sentence — reads as the
  notification catalogue's text for that key, with the key and its parameters
  kept beneath it; OpenSim's sentence is shown as it came. A progress key
  reads as the reference's text for it, and a line that is no key is shown as
  it came.
- A `TeleportStart` flagged `DISABLE_CANCEL` hides the display's Cancel
  button. Second Life's `TPCancelled` answer to a cancel closes the display
  rather than reopening it as a failure; a teleport OpenSim completes after a
  cancel is shown arriving.
- **The session's own deadline is reported as its own.** A teleport the grid
  neither carries out nor refuses within thirty seconds ends in the same
  `Event::TeleportFailed` a refusal does, with a
  `Diagnostic::ExpectedReplyMissing` for `Teleport` beside it: the reason is
  free text either way, and "the grid said nothing" is the answer Second Life
  gives a dead lure.
- `InstantMessage::lure_destination` decodes a Second Life offer's bucket: the
  region, the landing position, the facing and the rating.
- The viewer's offer card shows the destination's rating when the offer
  states one. Pressing Teleport on an offer rated above the user's preference
  is answered by the grid's refusal, shown as any refusal is; the reference
  asks first whether to raise the preference, which this viewer does not do
  yet (the roadmap's `viewer-teleport-offer-maturity-prompt`).
- A teleport request raises a card of its own — it raised nothing before this
  was measured. Offer Teleport answers with an offer to the requester;
  Decline sends nothing, there being nothing to send.
- `e2e_two_avatars` delivers each flavour's offer and a request to a viewer on
  both fake flavours and holds the cards and their answers to the above; its
  two-viewer test of an offer declined and an offer taken runs on a live grid
  (`SL_E2E_GRID=opensim|aditi`), and passed on both on 2026-10-07.
- **A retired neighbour is reported.** `Event::NeighborRetired` names the
  region, its simulator, the circuit and the reason — the simulator's
  `DisableSimulator`, a circuit gone silent, or a transfer abandoned. The
  session used to drop the circuit and its objects and say nothing of the
  region, so a mirror of the regions kept one the draw distance had stopped
  reaching until the next world reset; the Bevy plugin's region index now
  drops it, and its parcel overlay with it.
- **A repeated announcement is not news.** An `EnableSimulator` for a region
  already held and an `EstablishAgentCommunication` naming a seed already
  held raise no event. Second Life's five-second repeat used to have the
  driver POST the same seed every five seconds for as long as the avatar
  stood there.
- **`ViewerEffect`s are read on a child circuit too**: Second Life sends the
  effects of avatars in a neighbouring region down that region's circuit, and
  they were reported as unhandled.
- The viewer's border-crossing check (the scene stays where it was, and stays
  drawn) runs against both fake flavours.
- `e2e_pilot` drives one request — an agent set to General asking the world
  map for a Moderate region — against both fake flavours: refused with the
  catalogue's text on the one, carried out on the other.
