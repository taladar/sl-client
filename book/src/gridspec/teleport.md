# Teleport

How each grid carries out a teleport the client asks for, and how it refuses
one. The messages and the client's handover are described in
[Teleport](../content/teleport.md); offers and requests between avatars are
not covered here.

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
| the flags of the start, the lines and the finish | the kind of teleport and nothing else: `VIA_LOCATION` (`0x10`), `VIA_HOME` (`0x20`), `VIA_LANDMARK` (`0x08`) | `VIA_LOCATION` (`0x10`) | each flavour's |
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

Refusals not provoked here: an estate ban, a full region on Second Life, a
destination that never confirms the arrival (OpenSim's source words that as
`Problems connecting to destination …`; the fake grid sends `timeout_tport`
by each flavour's transport), and a lure whose host is gone
(the roadmap's `gridspec-teleport-lures`).

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
- `e2e_pilot` drives one request — an agent set to General asking the world
  map for a Moderate region — against both fake flavours: refused with the
  catalogue's text on the one, carried out on the other.
