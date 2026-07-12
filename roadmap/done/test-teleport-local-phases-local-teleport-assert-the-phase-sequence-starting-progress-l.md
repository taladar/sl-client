---
id: test-teleport-local-phases
title: local teleport; assert the phase sequence Starting → Progress → Landin
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 12 — Teleport (state machine) `[both]`
---

Context: [context/test.md](../context/test.md).

`teleport-local-phases` — local teleport; assert the phase sequence
Starting → Progress → Landing → Complete. `1av`. Teleports to the centre of
the agent's current region and collects the teleport phases the session
surfaces until arrival, asserting the sequence opens with *Starting*
(`TeleportStart`) and ends at a terminal phase — the intra-region
`TeleportLocal` for the expected local case (or a `RegionChanged` handover
tolerated for an avatar that logged in adjacent to the target). OpenSim's
local path emits only `TeleportStart` → `TeleportLocal` (no intermediate
`TeleportProgress` / distinct Landing frame — `SendTeleportStart` then
`SendLocalTeleport`), which is that grid's complete local sequence; recorded
green with `phase_sequence = "started,local"` and `progress_updates = 0`.
