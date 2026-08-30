---
id: test-fake-grid-determinism
title: An injectable clock and seeded ids for sl-fake-grid
topic: test
status: in-progress
origin: test-harness plan (2026-08-30)
points: 3
refs: [viewer-fake-grid-login-smoke]
---

Context: [context/testing.md](../context/testing.md).

In progress (2026-08-31): the id half landed — an `IdMinter`
(xorshift64, v4-shaped uuids) behind sessions, caps tokens, circuit
codes, accounts and regions, `FakeGridBuilder::deterministic(seed)`,
and a determinism test proving one seed repeats the agent and region
ids while a different seed does not. Remaining: threading the `Now`
clock through the fourteen `Instant::now()` sites, and the full
acceptance run below.

Every fake-grid path calls `Instant::now()` directly (fourteen sites in
the driver, teleport, runtime, scenario and caps endpoint) and mints
session ids, circuit codes, the CAPS seed and per-cap tokens with
`Uuid::new_v4()`. The sans-I/O `SimSession` takes `now` on every entry
point, so the core is clock-injectable — the grid simply never injects.

- `Now = Arc<dyn Fn() -> Instant + Send + Sync>` on `SharedSim` and
  `GridCore`; `FakeGridBuilder::clock(now)`; `SimHook` gains a `now`
  parameter. Under `tokio::time::pause()` a test passes the paused clock.
- `FakeGridBuilder::deterministic(seed)`: a seeded `IdMinter` (v4-shaped)
  feeding `SessionIds::mint`, `rand_circuit_code`, `SimCaps::new(..,
  mint_token)` (already a closure parameter), region and account ids.

Acceptance: two runs of the tokio end-to-end suite under
`deterministic(1)` produce identical decoded event sequences; UDP resend
timing is still client-driven, so tests assert decoded events, never raw
datagrams.
