---
id: test-full-stack-sigsegv-at-exit-under-load
title: A full-stack test passed and then its process died of SIGSEGV, once, in the full parallel suite
topic: test
status: bugs
origin: the ggh pre-commit nextest run for viewer-sliced-art-seam-at-fractional-ui-scale (2026-09-25)
refs: [viewer-sliced-art-seam-at-fractional-ui-scale]
---

Context: [context/test.md](../context/test.md).

## Observation

In the ggh pre-commit hook's workspace run (`cargo nextest run --target-dir
target/nextest_check --no-tests pass -- --include-ignored`, 6282 tests in
parallel), one process ended with signal 11 **after** its test had passed:

```text
SIGSEGV [   3.931s] (6263/6282) sl-client-bevy-viewer
  full_stack_test::tests::an_empty_own_appearance_asks_the_grid_to_rebake
  test full_stack_test::tests::an_empty_own_appearance_asks_the_grid_to_rebake
    ... ok
  test result: ok. 1 passed; 0 failed; …; finished in 1.95s
  (test aborted with signal 11: SIGSEGV)
```

So the crash is in **process teardown** — after the harness printed its
result — not in the test body.

## What did not reproduce it

- The test alone, `--stress-count 10`: 10 passes.
- Every `full_stack_test` plus the `sl-viewer-world-avatar` `gpu_avatar*`
  tests together, `--stress-count 4`: 4 passes.
- `coredumpctl` kept no core for it.

The commit it was found in changes only `bevy_ui_render`'s nine-slice corner
maths, which the headless full-stack scene does not draw.

## Where to look

A teardown crash that needs the whole suite's load points at the GPU stack
(wgpu / Vulkan device and surface drop order with many processes on the one
GPU) or a background thread (CEF, tokio, the fake grid) still running when
statics are destroyed. "Passes alone" has meant a real race before
(`sl-client-full-stack-teleport-test-flaky`).

- Enable core dumps for the nextest run (`ulimit -c unlimited`, or check why
  systemd-coredump kept nothing) so the next occurrence leaves a backtrace.
- Run the workspace suite under load several times to get a rate.

## Done when

The crash is explained from a backtrace and fixed, or shown to be in a
third-party teardown path and handled there.
