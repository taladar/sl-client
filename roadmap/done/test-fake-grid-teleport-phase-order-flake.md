---
id: test-fake-grid-teleport-phase-order-flake
title: teleport-cross-region intermittently sees a phase before TeleportStart
topic: test
status: done
origin: a ggh pre-commit nextest run during test-fake-grid-simulator-request-surfaces (2026-09-04)
points: 2
refs: [test-fake-grid-simulator-request-surfaces, test-fake-grid-teleport-shapes]
---

Context: [context/testing.md](../context/testing.md).

`sl-conformance::offline test::teleport_cross_region` failed under load with:

```text
Error: "teleport-cross-region: expected the teleport to begin with a Starting
(TeleportStart) phase"
```

## The mechanism

The two halves of a teleport reach the client on **two transports**, and the
client's driver picks between them at random.

- `TeleportStart` and the `TeleportProgress` lines go out over **UDP** on the
  source circuit (`sl-fake-grid/src/teleport.rs`, via `SimSession::send_*`).
- `EnableSimulator`, `EstablishAgentCommunication` and `TeleportFinish` go out
  over the **CAPS event queue** — HTTP, a different socket and a different
  client task (`SimSession::enqueue_*`).
- `sl-client-tokio`'s run loop selects over its UDP socket and its event-queue
  channel with a plain `tokio::select!`, which is **unbiased**: whenever both
  are ready, tokio picks one at random. Under load the loop can stall long
  enough to span both arrivals, and then it is a coin flip.

When the event queue wins, `Session::handle_caps_event`'s `TeleportFinish` arm
runs first. The session is already `Teleporting` (the client asked for this
teleport), so it pushes `Event::TeleportFinished` and calls `begin_handover` —
which deliberately leaves the state `Teleporting`. The late UDP `TeleportStart`
therefore still passes the state gate in the UDP arm and pushes
`Event::TeleportStarted` **after** the finish. The observed sequence is

```text
["finished", "started", "region-changed"]
```

which is exactly the assertion's complaint, and is pinned by
`a_caps_finish_ahead_of_the_udp_start_reorders_the_phases` in
`sl-proto/tests/lifecycle.rs`.

### What the first write-up of this got wrong

The original guess — that the destination's promotion reaches the client before
the source's `TeleportStart`, leaving `region-changed` as a single-entry
`phases` — is **not reachable**. `RegionChanged` is only produced by
`commit_handover`, which can only be armed by `begin_handover`, which runs in
the same handler that pushed `TeleportFinished` immediately before it. So
`finished` always precedes `region-changed`, and the race is not between two UDP
sockets: it is between one UDP socket and the HTTP event queue, inside the
client's own select loop.

## Why the fake grid and not a live one

The instinct in the first write-up was right here. On OpenSim the gap between
`TeleportStart` and `TeleportFinish` is the whole `EntityTransferModule`
handover — serialising the agent, posting it to the destination simulator,
waiting for that simulator to build it — tens to hundreds of milliseconds. The
fake grid's destination for this case is the *neighbour*, whose session is
already open (`core.session_of` hits), so between the two there is a mutex
acquire and a couple of enqueues: well under a millisecond. That is what makes
the client-side stall wide enough to span both.

## The fix

The grid now orders the two itself. `teleport_session` records the sequence
number the `TeleportStart` goes out as (`SimSession::next_outgoing_sequence`,
read before the send) and waits for the client's acknowledgement of exactly that
packet (`SimSession::is_awaiting_ack`) before it enqueues the CAPS trio. A
client only acknowledges a packet it has decoded and handled, so the ack is a
genuine happens-before edge: once it lands, `Event::TeleportStarted` is already
on the client's event stream and nothing the event queue delivers afterwards can
precede it.

Preparing the destination runs concurrently with the ack round trip, so the wait
usually costs nothing measurable — the thirteen `sl-fake-grid` teleport
end-to-end tests still finish in ~2.5 s together. The wait is bounded
(`TELEPORT_START_ACK_TIMEOUT`, 2 s) and aborts on grid shutdown; on expiry the
teleport proceeds and logs, because an ordering nicety must never be able to
strand a teleport.

Not taken: biasing the client's `select!` toward UDP. It would fix the coin
flip and replace it with a worse failure — on a busy region the UDP arm is
almost always ready, so a biased select would starve the event queue and drop
the `TeleportFinish` entirely. Also not taken: weakening the assertion to
presence rather than position, which would have given up a property that holds
on every live grid.

## What pins it

- `sl-proto/tests/sim_session.rs`
  `a_reliable_send_is_awaited_until_the_client_acknowledges_it` — the seam, and
  the distinction it rests on: delivering a datagram is not the client
  acknowledging it.
- `sl-proto/tests/lifecycle.rs`
  `a_caps_finish_ahead_of_the_udp_start_reorders_the_phases` — what the client
  does when the race is lost, which is what the grid-side edge exists to
  prevent (and confirms the handover itself survives it).
- `sl-fake-grid/tests/client_end_to_end.rs`
  `a_neighbour_teleport_starts_before_it_finishes` — the whole phase sequence
  end to end, collected in one wait and asserted as an order, on the neighbour
  teleport where the window is tightest.
- `sl-conformance::offline test::teleport_cross_region` — unchanged, and the
  case that reported the flake in the first place.
