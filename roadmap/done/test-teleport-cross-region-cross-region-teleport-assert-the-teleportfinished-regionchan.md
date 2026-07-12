---
id: test-teleport-cross-region
title: cross-region teleport; assert the TeleportFinished → RegionChanged han
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 12 — Teleport (state machine) `[both]`
---

Context: [context/test.md](../context/test.md).

`teleport-cross-region` — cross-region teleport; assert the
`TeleportFinished` → `RegionChanged` handover. `1av`. **Green on OpenSim.**
Discovers a neighbour via the world map ([`RequestMapBlocks`] over a
one-cell margin around the agent's own region) and teleports to the first
region whose grid coordinates differ from the current one — a genuinely
different destination that forces the cross-region path (a circuit retarget,
`UseCircuitCode`, `CompleteAgentMovement`) rather than the intra-region
`TeleportLocal`. Collects the teleport phases and asserts the sequence opens
with *Starting* ([`Event::TeleportStarted`]), carries a
[`Event::TeleportFinished`] naming the destination region handle, and ends at
a [`Event::RegionChanged`] to that same handle (a `TeleportLocal` fails the
case, since that would mean the teleport did not cross a boundary), then
confirms the session's current region handle is now the destination. Recorded
green teleporting Default `(1000,1000)` → East Region `(1001,1000)` across the
2×2 block (ports 9000/9001), with `phase_sequence =
"started,finished,region-changed"` and `progress_updates = 0`. **No new client
code** — the CAPS
`TeleportFinish` handover (`begin_handover`) and the map-block discovery path
already existed; the only harness change was making the conformance `Session`
keep `region_handle` / `circuit_id` current as it observes `RegionChanged`
(they were a frozen login-time snapshot before, so the post-teleport region
read stale). `[both]`; the aditi run is deferred with the batch (SL answers a
cross-region teleport with its own `TeleportFinish` handover to a distinct
destination simulator).
