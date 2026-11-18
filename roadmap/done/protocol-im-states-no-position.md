---
id: protocol-im-states-no-position
title: Every instant message the session sends states position zero
topic: protocol
status: done
origin: gridspec-teleport-lures (2026-10-07)
refs: [gridspec-teleport-lures, gridspec-instant-messages]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

`ImprovedInstantMessage` carries the sender's `Position`, and the reference
viewer fills it with the agent's position in its region for every instant
message it sends (`send_improved_im` and `pack_instant_message` with
`gAgent.getPositionAgent()`). The session sends `0, 0, 0` in all three of its
senders (`sl-proto/src/session/circuit.rs`: `send_instant_message_raw`,
`send_im` and the group-session one), because it keeps no record of where
its own agent stands.

OpenSim relays the field as it came: a teleport request sent by the
conformance harness arrived at the other avatar stating position zero
(`teleport-request`, 2026-10-07). Second Life's relayed request stated zero
as well, which may be our zero or its own — an offer, whose position the
simulator fills in, also arrived stating zero there, so Second Life may not
pass the field on at all.

## Why it matters

Nothing in this workspace reads the field of a received message, so nothing
is broken by it today. It is a place where the client is distinguishable
from a viewer on the wire, and a script or a third-party viewer that does
read it (the reference uses it for nothing but logging) is told the sender
is at the region's corner.

## Wanted

The session's own position — it already receives its avatar's object updates
and each `AgentMovementComplete` — kept and sent, in all three senders and in
both runtimes; or a decision, recorded here, that zero is what we send.

## Resolution (2026-11-18)

The premise was half right: the reference does **not** fill the field for
every instant message. Its conversational ones — an ordinary message and a
session's messages (`deliverMessage`), typing, leaving a session and the
do-not-disturb reply — go through `pack_instant_message`'s default
arguments and state zero; everything else (`send_improved_im`, the offer
answers, giving inventory, a teleport request, a group or conference session
start) states `gAgent.getPositionAgent()`.

The session now does the same: `ImDialog::states_sender_position` holds the
rule, and `Session::own_region_position` reads the agent's own avatar object
on the root circuit, composed through the seat it sits on, or the last
`AgentMovementComplete`'s position before that object arrives. All three
circuit senders take the position the session chose, so both runtimes send
it with no change of their own.

Live on 2026-11-18 (`teleport-request`): OpenSim passes the request's
position on as sent (`128.00 128.00 30.00`, where it was zero); Second Life
states zero for a request sent with the requester's position, so it does not
pass the field on. The fake grid's Second Life flavour now states zero for a
request too.
