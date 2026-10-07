---
id: protocol-im-states-no-position
title: Every instant message the session sends states position zero
topic: protocol
status: bugs
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
