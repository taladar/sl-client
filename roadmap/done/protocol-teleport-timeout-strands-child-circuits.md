---
id: protocol-teleport-timeout-strands-child-circuits
title: A timed-out teleport strands the session and loses its child circuits
topic: protocol
status: done
origin: user report (2026-08-07), live teleport testing on the local 2x2 grid
refs: [viewer-seamless-region-handover-objects, protocol-teleport-deferred-teardown-handover]
---

Context: [context/viewer.md](../context/viewer.md).

Live symptom (user): after a teleport times out and is dismissed, the retry
teleport to a **neighbouring** region succeeds but the destination then has **no
child circuits** — and no crossing works from there. The user's read (correct):
the timeout tears the child circuits down and the retry falls into
distant-teleport (fresh-circuit) logic, and nothing re-establishes the
neighbours afterward.

## Root cause (confirmed by code trace)

`begin_handover` commits to the destination *before it confirms*: on
`TeleportFinish` it either **retargets** the root circuit to the destination
(fresh branch — `Circuit::retarget` also wipes `children`/objects/terrain) or
**promotes** a child and demotes the old root (neighbour branch), then moves to
`SessionState::AwaitingHandshake`. There is then **no recovery path**:

- The teleport timer + its `run_timeout` check only fire while state is
  `Teleporting`; after `begin_handover` the state is `AwaitingHandshake`, which
  has **no timeout** (and the fresh branch's `retarget` cleared the teleport
  timer anyway).
- `cancel_teleport` only resets from `Teleporting`, so the viewer watchdog's
  `Command::CancelTeleport` — whose comment says it "recover[s] the session so
  further teleports are accepted" — is a **no-op** once in `AwaitingHandshake`.
- `teleport_to` rejects any state other than `Active`/`Teleporting` with
  `NotActive`, so the next teleport is refused outright.
- The only backstop is the root circuit's 45 s `inactivity` **disconnect**, and
  only if the destination goes completely silent; any packet refreshes it and
  the session is stuck indefinitely.

Downstream: because the handover never produced a clean `RegionChanged`, the
driver (`sl-client-tokio` / `sl-client-bevy`) never re-fetches caps / restarts
the CAPS event queue for the new region, so no `EnableSimulator` arrives and no
neighbour child circuits re-open — exactly the observed "no child circuits."

Fixed by [[protocol-teleport-deferred-teardown-handover]] (defer the teardown
until the destination confirms, so a failed teleport leaves the source region
and its children fully intact).
