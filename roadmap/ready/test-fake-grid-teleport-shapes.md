---
id: test-fake-grid-teleport-shapes
title: The teleport matrix — same region, neighbour, distant, and each one failing
topic: test
status: ready
origin: review of test-fake-grid-neighbours-crossing (2026-09-03)
points: 5
refs: [viewer-fake-grid-teleport, test-fake-grid-neighbours-crossing, protocol-teleport-timeout-strands-child-circuits]
---

Context: [context/testing.md](../context/testing.md).

A teleport's destination comes in three shapes, and the grid does something
**structurally different** for each: the same region is answered in place
with a `TeleportLocal` and no second session at all; a region already in
the agent's neighbour set reuses the child circuit the client is holding;
anywhere else opens a session on the spot. The tests cover the shapes
unevenly and the failure half almost not at all.

## What holds today

- `same_region_teleport_is_local` — the local hop.
- `inter_region_teleport_over_loopback` — the distant hop, end to end
  (progress keys, the event-queue trio, the arrival, the source retired).
- `grid_initiated_teleport_lands_the_client`, the landmark / home / lure
  resolvers, and the three refusal keys (`invalid_tport`,
  `nolandmark_tport`, `no_host`).

## The gaps

**The neighbour shape is untested.** Reusing an announced child session is
new code ([[test-fake-grid-neighbours-crossing]]) and nothing exercises
it. Assert: exactly **one** live session in the destination region for the
agent afterwards (not two), one simulator address announced for that
region handle, the destination's objects delivered once rather than twice,
and the arrival placed where the request asked even though the session was
built long before with a placement of its own.

**No shape is tested failing.** `Error::TeleportTimedOut` — the client
never sends its `CompleteAgentMovement` — is unexercised, and the cleanup
it runs is *shape-dependent*, which is exactly the kind of thing that
rots:

- a **fresh** destination is removed from the session table and abandoned;
- a **reused neighbour child** must be left alone — it is still a
  neighbour and the client still holds its circuit, and tearing it down
  would punish it for someone else's failed teleport;
- either way the source must be told `timeout_tport` and stay the root
  agent, with its own neighbour set intact.

A same-region hop has no timeout to test (there is no second session to
wait for); say so in the test file rather than leaving the reader to
wonder which cell of the matrix is missing.

Also worth a case: **teleport away and back**. Returning to a region the
agent left should re-announce that region's neighbours from scratch rather
than inheriting a stale set, and the region left behind should be retired
or re-announced consistently.

## Shape

The tokio tier (`sl-fake-grid/tests/client_end_to_end.rs`), because every
claim here is about sessions, circuits and event ordering rather than
pixels. Provoking the timeout needs a client that opens the destination
circuit but never completes its movement — drive the destination's
`SimSession` by hand rather than through the real client, or park the real
client's arrival, whichever reads more honestly.

`TELEPORT_ARRIVAL_TIMEOUT` is 30 s, so the failure cases want either an
injected clock (`FakeGridBuilder::clock` + a paused tokio timer, as
`tests/clock.rs` does) or a shorter budget made configurable — do not make
the suite wait out the real one.
