# Circuits

A **circuit** is one agent's UDP connection to one simulator. Where a
[session](sessions.md) is the logical "I am logged in" relationship, a circuit
is the concrete transport endpoint: a remote `SocketAddr`, a sequence counter,
the reliability bookkeeping, and the circuit code that authorizes it. Everything
in the [LLUDP transport](lludp-transport.md) chapter is per-circuit state.

## What a circuit is

A circuit bundles:

- the simulator's UDP address,
- the **agent id** and **session id** it is bound to,
- the **circuit code** (a 32-bit integer) that the region uses to recognize it,
- the outgoing **sequence number** counter,
- the **reliability state**: unacknowledged reliable packets awaiting resend,
  the queue of acks to send back, and the window of recently-seen incoming
  sequence numbers.

One agent can hold several circuits at once (see
[multiple circuits](#multiple-circuits-and-region-crossing) below), but exactly
one is the **root** circuit — the region the avatar is actually standing in.

## Establishing a circuit

The circuit code is issued by the [login](../content/login.md) service (or, for
a neighbouring region, by the current region). Bringing a circuit up is a short
handshake:

```text
client ──▶ UseCircuitCode { Code, SessionID, AgentID }   (reliable)
client ──▶ CompleteAgentMovement { AgentID, SessionID, CircuitCode }
region ──▶ RegionHandshake { … region identity … }       (reliable)
client ──▶ RegionHandshakeReply
            … now Active; ObjectUpdate / terrain / etc. begin flowing …
```

1. **`UseCircuitCode`** tells the region "this UDP endpoint, with this circuit
   code, belongs to this agent/session." Until the region sees this, it ignores
   the datagrams.
2. **`CompleteAgentMovement`** asks the region to actually place the avatar (or
   complete the handover from another region).
3. The region replies with **`RegionHandshake`**, which carries the region's
   identity (name, flags, maturity, owner, water height, …). The client answers
   with **`RegionHandshakeReply`**, and the session becomes
   [`Active`](sessions.md#the-lifecycle).

`UseCircuitCode` is sent reliably; if its retransmissions are exhausted the
session reports a handshake failure.

## Multiple circuits and region crossing

A grid is a grid of regions, and an avatar near a region border can see into the
neighbours. To stream objects and terrain from those neighbours, the client
opens **child circuits** to them — additional circuits, each with its own
sequence counter and reliability state, but subordinate to the root.

When the avatar crosses into a neighbour:

- the region announces the neighbour ahead of time (an `EnableSimulator` message
  and/or an `EstablishAgentCommunication` event carrying the neighbour's seed
  [capability](caps.md)),
- the client establishes a child circuit there,
- and on the actual crossing the child is **promoted to root**.

A [teleport](../content/teleport.md) to a distant region is similar but
deliberate rather than incidental. Either way, the multi-circuit machinery is
what makes seamless movement across a contiguous grid possible.

A circuit outlives the region it was opened for. The reference viewer never
tears one down when it drops a region, because its `disableCircuit` finds no
circuit code on the viewer side, so it goes on acking, answering pings and
pinging the region the agent left. A teleport back to that simulator reuses the
same circuit, and its sequence numbers continue. On Second Life, a client that
dropped the circuit instead and teleported back about a minute later found the
first visit's capabilities revoked (404), and the region answered no further
teleport request.

How each grid pings a circuit, resends on it and gives it up is measured in
[Grid Behaviour → Session](../gridspec/session.md#circuits).

> **Note (`AddCircuitCode`).** There is also a server-to-server `AddCircuitCode`
> message used between simulators to pre-authorize an incoming agent's circuit.
> A client never sends it, but you will see it in the message template.

---

> **In this codebase**
>
> - A circuit is the `Circuit` type in `sl-proto/src/session/circuit.rs`. It
>   holds a `ReliableLink` (`sl-proto/src/link.rs`) for the sequence counter and
>   the resend/ack/seen-window bookkeeping — the same type the server's
>   `SimSession` holds, see [LLUDP transport](lludp-transport.md) — and exposes
>   the `send_use_circuit_code`, `send_complete_agent_movement`, and
>   `send_region_handshake_reply` helpers used during the handshake.
> - The owning `Session` (`sl-proto/src/session.rs`) holds the root circuit and
>   any child circuits, and handles promotion on region change. A distant
>   teleport moves the old root and its children into `Session::departed`.
>   There they are kept at the transport level only (acks, pings, resends,
>   and `DisableSimulator` to end one), and the next circuit to the same
>   simulator takes one back under a fresh circuit id. A departed circuit that
>   stays silent past the inactivity timeout is dropped.
> - A request **about an object** goes to the region the object is in, not
>   to the root: a `ScopedObjectId` names its circuit directly, and an
>   object-key request (`RequestObjectPropertiesFamily`, `RequestPayPrice`,
>   the spin / grab updates, the script running / reset requests,
>   `BuyObjectInventory`) is routed through `Session::circuit_for_object`, the
>   reference's `objectp->getRegion()->getHost()`. The replies — and a
>   neighbour script's `ScriptDialog` / `ScriptQuestion` — are handled on child
>   circuits too, and a script's answer goes back to the circuit its request
>   arrived on (`Session::circuit_for_script_reply`).
> - `UseCircuitCode`, `CompleteAgentMovement`, `RegionHandshake`,
>   `RegionHandshakeReply`, and `AddCircuitCode` are generated
>   [messages](messages.md) in `sl-wire`.
> - The neighbour/handover events are `Event` variants in
>   `sl-proto/src/types/event.rs` (look for the `EnableSimulator` /
>   `EstablishAgentCommunication` / region-change handling).
> - The seed rides the event queue and the retirement (`DisableSimulator`) rides
>   UDP, so a seed can arrive after its child is gone. The session drops a seed
>   for a simulator it has no circuit to, as Firestorm drops one for an unknown
>   region. Kept, it would make the neighbour's identical seed look already
>   known when the neighbour came back, and that child's capabilities would
>   never be fetched. Second Life repeats the event every few seconds, so a
>   neighbour that is still there loses nothing.
> - A `KillObject` is remembered for a minute with the sequence number of the
>   packet that carried it. A resent packet keeps its original number, so an
>   object update numbered below the kill was sent while the object still
>   existed: it is dropped, and so is a cache probe or terse update that would
>   ask for the object again. An update numbered above the kill is the object
>   coming back and is taken.
