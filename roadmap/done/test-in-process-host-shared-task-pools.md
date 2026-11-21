---
id: test-in-process-host-shared-task-pools
title: In-process viewers share Bevy's task pools, so one stuck viewer can stall another on a small machine
topic: test
status: done
origin: verifying the in_process_host test deadlines under a CPU quota (2026-11-21)
refs: [test-unbounded-waits-in-test-harnesses, test-conformance-object-asset-format-fails-under-load]
---

Context: [context/test.md](../context/test.md).

`sl-viewer-automation`'s `InProcessHost` runs each viewer's App on a thread of
its own. Its tests claim that a viewer stuck in a frame does not stall another
(`a_viewer_stuck_in_a_frame_does_not_stall_another`). That holds only while
Bevy's task pools have threads to spare.

## Observed

Run CPU-starved with
`systemd-run --user --scope -p CPUQuota=15%` (and again at `200%`), with every
deadline other than the one under test at 60 s:

- `a_viewer_stuck_in_a_frame_does_not_stall_another`:
  `viewer running did not finish being built within 300s`
- `stopping_gives_up_on_a_thread_stuck_in_a_frame`:
  `viewer running did not finish being built within 60s`

The same tests pass at full parallelism, alone and in five stress iterations
against 48 `yes` CPU hogs on the 24-core machine. So this is not slowness: the
second viewer's build never finishes at all.

## Why (code reading, not yet confirmed by instrumentation)

- Bevy's `ComputeTaskPool`, `AsyncComputeTaskPool` and `IoTaskPool` are
  process-wide statics, so every App in one process shares them.
- Their size follows `std::thread::available_parallelism`, which on Linux reads
  the cgroup CPU quota. 15% or 200% leaves a pool of about one compute thread.
- The test's `stall` system blocks inside a frame, so it holds a compute thread.
  The second viewer's build calls `settle`, which steps frames. Its
  multi-threaded executor then waits for a compute thread that never frees.

## Why it matters beyond the tests

On a machine or a CI runner with few cores, or under a container CPU limit, one
in-process viewer stuck in a frame would stall every other in-process viewer of
the stage. The host's deadlines would turn that into `TimedOut` errors rather
than a hang, but the second viewer would fail for the first one's fault.

## To decide

- Confirm the pool hypothesis, for example by logging the pool sizes under the
  quota, or by sizing the pools explicitly through `TaskPoolPlugin`.
- Then choose: give the host a guaranteed minimum pool size, run each viewer's
  executor single-threaded, or document the limit and make the test say which
  pool size it needs.

## Resolution (2026-11-21)

Confirmed. Bevy's multi-threaded executor calls `scope_with_executor(false, …)`,
so the calling thread never runs tasks of the global pool, and a system blocked
on the only compute thread stops every other App. CPU affinity reproduces it
without the slowness of a quota: under `taskset -c 0` or `0,1` (one compute
thread) the second viewer's build never finished, under `0-3` (two) it did.

`InProcessHost::host` now creates the pools before the first viewer's build,
with Bevy's sizing except for at least `MIN_COMPUTE_THREADS` (8) compute
threads. A pool is created once per process, so one built earlier cannot grow,
and the host warns in that case. The guarantee is a number rather than a proof:
eight or more systems stuck at once across all viewers would still stall the
rest.

Verified: the stuck-viewer, stop and task-timeout tests pass under `taskset` at
one, two and four CPUs, and the first two at `CPUQuota=15%` and `200%`.
