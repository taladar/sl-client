---
id: test-full-stack-sigsegv-at-exit-under-load
title: A full-stack test passed and then its process died of SIGSEGV, once, in the full parallel suite
topic: test
status: done
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

## Second occurrence — with a core (2026-09-29)

The pre-commit run for `test-e2e-viewer-process-launch` hit it again, in
`automation_executor::full_stack::a_session_is_driven_over_the_automation_socket`
(passed in 7.97 s, the process died at 23.4 s). Alone, `--stress-count 8`: 8
passes. This time `coredumpctl` kept the core, and the two threads involved are:

- **The main thread** had returned from `main` and was in `exit()` →
  `__run_exit_handlers` → the Vulkan validation layer's atexit
  `vvl::FreeAllDispatchObjects`, destroying its `DispatchDevice` /
  `DeviceState` (command pools, command buffers).
- **A Bevy task-pool worker** was at the end of the detached async block in
  `bevy_render::view::window::screenshot::collect_screenshots`
  (`screenshot.rs:717`), dropping the block's clone of the readback
  `bevy_render::render_resource::Buffer`. That was the **last reference to the
  wgpu `Device`**, so the drop ran `wgpu_hal::vulkan::Device` teardown →
  `gpu_allocator` → `vkFreeMemory` through the layer chassis, which jumped into
  unmapped code: `SIGSEGV`.

So a screenshot readback task outlives the App that asked for it. The harness
dropped the App, but the detached task still held the buffer (and through it
the device), and it finished only after `main` returned, while the layer's
exit handler was tearing down the state that the device drop called into.
Load widens the window, because the worker runs late. The validation layer is
present because wgpu enables it in debug builds, and the hook's run is a
debug build.

## Fix (2026-09-29)

In the Bevy fork (`9eb8d6fa`, `bevy_render` `screenshot.rs`):
`collect_screenshots` no longer detaches its tasks. It keeps them in a
render-world resource, `RenderScreenshotTasks`, which drops the finished
ones each frame. When the resource is dropped (with the render world, so with
the App), it `block_on`s `Task::cancel()` for each unfinished one, and
`async-task` returns from that only once the future has been dropped or has
completed. So no readback task, and none of its buffers, outlives the App
that started it. That holds for every App — the harnesses, the in-process
transports and the binary — without a teardown wait in each of them.

The first version of this turned the leak into an abort: cancelling a task
whose map was still pending dropped its buffer, wgpu ran the pending map
callback with an error from inside that drop, and upstream's callback
`panic!`s on an error, which aborts the process. Six off-screen-window
full-stack tests died of `SIGABRT` at teardown. The callback now hands its
result to the task. A live task logs a real map failure, and a cancelled one
has no receiver left, so the result is dropped with it.

This fixes the teardown path the core shows. The crash was never
reproducible on demand (twice, in full parallel suite runs), so a green hook
is the evidence available, not a deterministic test.
