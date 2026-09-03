---
id: test-fake-grid-teleport-shapes
title: The teleport matrix — same region, neighbour, distant, and each one failing
topic: test
status: done
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

## Done (2026-09-03)

Five tests in the tokio tier, and two grid changes they needed.

`FakeGridBuilder::handover_timeout` overrides both arrival budgets
(`TELEPORT_ARRIVAL_TIMEOUT`, `CROSSING_ARRIVAL_TIMEOUT`). The failure
cases use 250 ms. It changes only how long the grid is *willing* to wait,
never what it does when the wait ends — the injected clock was the other
option, but pausing tokio's timer under a real client on real sockets
means auto-advance can fire the budget in the middle of a UDP round trip.

`FakeGrid::sessions_in(region)` lists the live sessions in a region, root
and child alike. Every claim in this task is about *how many sessions
exist where*, which is invisible from the client side; without it the
tests would have been inferring structure from event traffic.

- `a_teleport_to_a_neighbour_reuses_its_child_session` — the untested
  shape. The `TeleportNotice`'s `to_seq` is the seq the *announcement*
  opened, one session in the destination afterwards rather than two, and
  the arrival where the request asked rather than where the child was
  built.
- `a_local_teleport_opens_no_second_session` — the cell with nothing to
  hand over.
- `a_teleport_that_never_arrives_abandons_a_fresh_destination` and
  `…_leaves_a_neighbour_child_alone` — the same failure two destination
  shapes apart, with opposite cleanups.
- `a_teleport_retires_the_children_of_the_region_left_behind`.

### A bug the matrix found

**A teleport never retired the children of the region it left.** A
crossing has always called `retire_distant_children`; a teleport retired
only the source, so an agent hopping across the grid left one open circuit
per region it had ever bordered, each still streaming to a client now
nowhere near it. `teleport_session` now retires them too, keeping the
destination's own neighbours — which its arrival announces.

### On provoking a timeout

The client is stopped (`run.abort()`) so its movement can never complete;
that is the one way to hold a handover open deterministically. It means
these tests assert the **grid-side** cleanup and not the `timeout_tport`
key reaching the client, because a stopped client cannot report what it
was told. That the failure-reporting path works end to end is
`teleport_to_unknown_region_is_refused`'s claim (`invalid_tport` observed
at a live client), and it is the same `report_failure`.

A live client that cannot arrive is not expressible here: on loopback it
arrives in under a millisecond, so any budget short enough to beat it is
short enough to race the announcement that precedes it.

### Verified by mutation

Each shape-dependent claim was checked by breaking the behaviour and
confirming the test fails — a green test against correct code proves
little. Forcing a fresh destination fails the reuse test; removing the
retirement fails the retirement test; abandoning a borrowed neighbour on
timeout fails the leave-it-alone test. In each case only the matching
tests failed.
