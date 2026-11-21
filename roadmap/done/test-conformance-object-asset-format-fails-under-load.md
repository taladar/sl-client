---
id: test-conformance-object-asset-format-fails-under-load
title: object_asset_format fails only in a full-workspace run, and fails early
topic: test
status: done
origin: full-workspace sweep during viewer-skin-checkbox-radio-shape (2026-09-22)
refs: [viewer-full-stack-teleport-leftover-race,
  viewer-render-readback-texture-anim-test-flaky, test-fake-grid-lsl-offline-cases]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-conformance::offline test::object_asset_format` failed one
`cargo nextest run --workspace` and has not failed since — **including a second
full-workspace run of the same tree** (6211 passed), which is the only
configuration that has ever shown it. So it is intermittent even in the one
place it happens, and a single green sweep is not evidence that it is gone.

## What is known

- **It fails *faster* than it passes.** 7.038 s in the failing sweep; 11.2 s
  alone and 11.9 s running the whole `--test offline` suite. So it is not a
  test-level timeout being overrun — something inside gave up or errored early.
- **Only under full-workspace parallelism.** `-p sl-conformance --test offline`
  passes 34/34 as a package; the single test passes alone. The workspace run
  is the only configuration that has ever shown it.
- **Not the skin work that was in flight.** `cargo tree -p sl-conformance`
  contains no `bevy_flair` at all, and nothing else touched that session
  reaches this crate. That is a dependency fact, not an argument from
  plausibility.
- **Each offline case starts its own `sl_fake_grid` on ephemeral ports**
  (`sl-conformance/tests/offline.rs` — one grid per test, deliberately, so a
  case that mutates the region cannot decide what the next one sees). Under a
  full-workspace run there are far more concurrent tests than under `-p`, so
  port pressure and scheduling latency are both much higher.

## What is *not* known, and why

**The failure message was thrown away.** The sweep was piped through
`tail -12` *inside* the command, so nextest's failure block never reached the
log — only the summary line survived. Re-running the sweep with the full output
captured is step one, and the lesson is the one already recorded for
`cargo test`: do not pipe a test run through `tail` when the failure detail is
the thing you need.

## Why "flaky" is not an acceptable answer here

Two entries in this roadmap started as "passes alone, must be a flake" and both
were real:

- [[viewer-full-stack-teleport-leftover-race]] — the test sampled a
  postcondition nothing waited for.
- [[viewer-render-readback-texture-anim-test-flaky]] — genuine GPU
  serialisation under load.

"Passes in isolation" is how a race presents, so it is evidence *for*
investigating rather than against.

## How to reproduce

```sh
# the only configuration that has shown it; keep the whole output
roadmap/coord.sh heavy --exclusive -- cargo nextest run --workspace >sweep.log 2>&1
grep -B 40 'FAIL.*object_asset_format' sweep.log

# and if it will not reproduce that way, raise the pressure deliberately
cargo nextest run -p sl-conformance --test offline \
  --test-threads 32 --run-ignored all
```

## Where to look once it reproduces

- The case body in `sl-conformance/src/cases.rs` (`object-asset-format`) — what
  it waits for, and whether any wait is a fixed duration rather than a
  condition. A fixed sleep that is generous on an idle machine is exactly what
  fails early under load.
- `run_offline_case`'s login / logout bracket: whether the 7 s point
  corresponds to a handshake step giving up.
- `sl_fake_grid`'s ephemeral port binding — a collision or a bind retry would
  surface as an early error rather than a hang.

## Done when

The failure is either reproduced and fixed at its cause, or shown to be
environmental with the evidence recorded here — not closed on the grounds that
a narrower run passes.

## Investigation (2026-11-20)

**Not reproduced**, in three configurations, all on release builds with every
line of output kept:

| run | result |
| --- | --- |
| `-p sl-conformance --test offline --test-threads 32` | 190/190 passed |
| `cargo nextest run --workspace` (the failing configuration) | 8736/8736 passed; the case 5.8 s on `fake_sl`, 10.7 s on `fake_opensim` |
| the case alone, `--stress-count 10` under `systemd-run CPUQuota=15%` | 20/20 passed, 15-20 s each |

**The timing places the failure at the start of the second flavour.** On
2026-09-22 (`82321a95`) one test ran both flavours in sequence: `grids()` lists
`FakeSl` first, which takes ~5.8 s, so a failure at 7.04 s came about a second
into the `FakeOpensim` run. That is the grid start, the login and the first
thing the case does after `wait_for_region`, which is
`session.cap("ViewerAsset")`. Its failure is the immediate assertion
`no ViewerAsset capability offered`.

**Code reading shows a race there.** `sl-client-tokio` delivers the region's
capability map to the caps reporter before it releases `CompleteAgentMovement`,
so no arrival event precedes it. But `context::Session` installed the map from a
spawned task of its own, separate from the task that forwards events, and
nothing ordered the two. A case could take `RegionHandshakeComplete` off its
channel while the drain task had not run yet, and find the map empty. Each
offline test runs its grid and client on a two-worker runtime, so under
full-workspace load the window is real, if narrow.

## Fix

`context::forward_session`: one task now forwards the events and installs both
the region's and the neighbours' maps, and a `biased` select polls the map
channels before the event channel. A map sent before an event is therefore
installed before that event is forwarded, by construction rather than by
scheduling luck. Unit tests cover a map queued before the arrival event being in
place when the case reads it with no yield in between, the neighbour maps and a
replacing region map, and closed reporters neither ending the forwarder nor
stopping events. The book's runner page says so.

**What this does not establish:** the original failure message is gone, so the
race is the cause the timing points to, not one a log confirms. If the case ever
fails again, the message will say whether it is this.
