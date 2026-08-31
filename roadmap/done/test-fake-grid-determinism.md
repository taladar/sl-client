---
id: test-fake-grid-determinism
title: An injectable clock and seeded ids for sl-fake-grid
topic: test
status: done
origin: test-harness plan (2026-08-30)
points: 3
refs: [viewer-fake-grid-login-smoke]
---

Context: [context/testing.md](../context/testing.md).

Done (2026-08-31). The id half: an `IdMinter` (xorshift64, v4-shaped
uuids) behind sessions, caps tokens, circuit codes, accounts and
regions, `FakeGridBuilder::deterministic(seed)`. The clock half: a
`time` module holding `Now`, `system_clock` and `tokio_clock`, the
clock on `GridCore` and every `SharedSim` (`SharedSim::now`,
`FakeAgent::now`), `FakeGridBuilder::clock(now)`, both hook types
carrying the stamp, and every `Instant::now()` site in the driver,
teleport, runtime, scenario and caps endpoint drawing from it. The one
remaining direct call is `wait_for_arrival`'s
`tokio::time::Instant::now()`, which is the timer's own clock and
already moves under `tokio::time::pause()`.

`tests/clock.rs` is the teeth: a grid on a clock skewed an hour ahead
stamps every scenario hook an hour ahead and holds an
already-expired-looking `EventQueueGet` poll open, while the same runs
on the stock clock do neither — both fail if any site reverts to
`Instant::now()`. `tests/determinism.rs` is the acceptance run: two
scripted login-to-chat sessions against `deterministic(1)` agree on the
minted agent, region, session and secure-session ids, the circuit code,
the seed and granted capability paths, and the decoded grid-side event
sequence (cadence events — `AgentUpdate`, pings, throttle, reliable
give-up — dropped, since UDP resend timing is the client's).

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
