---
id: test-conformance-object-asset-format-fails-under-load
title: object_asset_format fails only in a full-workspace run, and fails early
topic: test
status: bugs
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
