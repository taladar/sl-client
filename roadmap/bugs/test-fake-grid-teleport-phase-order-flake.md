---
id: test-fake-grid-teleport-phase-order-flake
title: teleport-cross-region intermittently sees a phase before TeleportStart
topic: test
status: bugs
origin: a ggh pre-commit nextest run during test-fake-grid-simulator-request-surfaces (2026-09-04)
points: 2
refs: [test-fake-grid-simulator-request-surfaces, test-fake-grid-teleport-shapes]
---

Context: [context/testing.md](../context/testing.md).

`sl-conformance::offline test::teleport_cross_region` failed once under the
pre-commit's `cargo nextest` run of the whole workspace suite:

```text
Error: "teleport-cross-region: expected the teleport to begin with a Starting
(TeleportStart) phase"
```

It has not reproduced since: five isolated `nextest` runs of that one case
passed, and a full 4459-test `nextest run` of the workspace passed it too. It is
therefore **load-sensitive and intermittent**, and it is not new — the case has
been in `fake::OFFLINE_CASES` since [[test-audit-fake-grid-conformance-grid]].
What changed around it is that the offline suite grew from sixteen cases to
twenty-one, so five more fake grids now run concurrently under nextest, which
plausibly raised the odds rather than created them.

The assertion now reports **which phases it did observe**
(`the phases observed were [...]`), because a bare "it was not started" sends
the next reader back to re-run it to find out; the next occurrence will name the
culprit and settle the question below.

## The likely mechanism

The phases the case collects do not all arrive on one circuit:

- `TeleportStart` and the two `TeleportProgress` lines go out on the **source**
  session, queued in one `with_sim` flush before the destination is touched
  (`sl-fake-grid/src/teleport.rs`);
- `RegionChanged` comes from the **destination** session's handshake — and this
  case's destination is the east region, which is announced as the start
  region's neighbour, so the client *already holds a child circuit to it* and
  the handover is a promotion rather than a fresh connection.

Two UDP flows on two sockets have no relative ordering. On a live grid the gap
between "the simulator says the teleport is starting" and "the destination
region is ready" is tens of milliseconds of real work; on the fake grid it is
microseconds, so under load the destination's promotion can reach the client
before the source's `TeleportStart` datagram is processed — leaving
`region-changed` as the first phase, which is exactly the shape of the failure
(the loop breaks on `Changed`, so `phases` would be a single entry).

A drop-and-retransmit on the source circuit would produce the same symptom with
`progress` first, but that one looks impossible from the client side: the
`TeleportProgress` arm only emits while the session is already `Teleporting`,
which is the same gate `TeleportStart` passes.

## Candidate fixes, once the diagnostic names the phase

- **Order the handover behind the screen.** Have the fake grid's teleport wait
  for the client to *ack* the `TeleportStart` + progress packets before
  promoting the destination. Faithful — a simulator does finish putting the
  screen up before handing over — and it removes the race rather than making it
  rarer. Needs a "wait until these sequence numbers are acked" seam the driver
  does not have.
- **Weaken the assertion to presence, not position.** Defensible on the wire
  (nothing orders two circuits against each other) but it gives up a property
  that holds on every live grid, so prefer the first.

Acceptance: the case's phase order is decided by something other than load —
either the grid orders the two flows, or the case documents why it cannot — and
the reasoning is written down where the next reader of a red pre-commit finds
it.
