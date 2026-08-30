---
id: viewer-render-gpu-serialisation
title: GPU tests stay in pre-commit — serialised, settled, deterministic
topic: viewer
status: ready
origin: test-harness plan (2026-08-30); closes the readback flakiness bug
points: 5
refs: [viewer-render-readback-texture-anim-test-flaky, viewer-render-readback-tier]
---

Context: [context/testing.md](../context/testing.md).

The readback tier runs inside the ggh pre-commit `nextest` on the dev
GPU, in parallel with other test binaries, under concurrent build load —
and flakes ([[viewer-render-readback-texture-anim-test-flaky]]). The user's
decision is to keep GPU tests in the hook, so the fix is determinism and
serialisation, not exclusion.

- `.config/nextest.toml`: a `gpu` test-group with `max-threads = 1`, an
  override assigning `render_readback::`, `render_matrix::`,
  `full_stack_test::` and `world_test::gpu_` tests to it with a generous
  slow-timeout, and an opt-in `gpu-full` profile for the all-pairs sweep.
  ggh runs plain `cargo nextest run` from the workspace root and honours
  this file. Add `gpu_lock()` for plain `cargo test`.
- `build_readback_app` disables `PipelinedRenderingPlugin`: frame counting
  becomes exact and the render world's logs reach `capture_logs()`, so the
  LogCapture universal (no WARN/ERROR while a scene runs) can be folded
  into every capture — the check that would have caught R26.
- Replace the fixed warm-up constants with `settle(app, &captured,
  Settle { min_frames, max_frames, filter_frames })`: a frame came back
  (the no-adapter detector by outcome, unchanged), no pipelines pending
  (a main-world `PipelineStatus` copied from the pipeline cache), every
  live probe rig completed a burst (`ProbeCaptureStats` written where the
  capture schedule rolls) plus a *measured* `filter_frames` for Bevy's
  environment-map filter, timeline reached — then hold the clock
  (`ManualDuration(ZERO)`) for the captured frame so `globals.time`
  shaders are frozen. A settle that never holds is a real failure with a
  report, not a flake.

Acceptance: the existing readback tests pass ten times in a row under the
hook while a `cargo build` runs concurrently.
