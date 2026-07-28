---
id: viewer-profiling-logplugin-tracing
title: Custom tracing subscriber blocks Bevy's trace_tracy / trace_chrome profilers
topic: viewer
status: done
origin: discovered 2026-07-25 while profiling the custom-face-material perf regression
refs: [viewer-profiling, viewer-custom-face-material-shader]
---

Context: [context/viewer.md](../context/viewer.md).

The viewer **disables Bevy's `LogPlugin`** and installs its **own**
`tracing_subscriber` from the binary (`lib.rs` `run_session`
`.disable::<LogPlugin>()`, `init_tracing`), because login logging happens
**before** the Bevy `App` / window exists and Bevy's `LogPlugin` only sets up
logging once the `App` runs.

**Side effect (the problem):** Bevy's built-in profilers attach their tracing
layers **through `LogPlugin`** — `--features bevy/trace_tracy` (Tracy) and
`bevy/trace_chrome` (Perfetto/Chrome). With `LogPlugin` disabled, those layers
never install, so a `trace_chrome` build produces **no trace file** and Tracy
sees nothing (confirmed 2026-07-25: two clean captures produced zero output).
This is exactly the tooling the [[viewer-profiling]] plan-of-record leans on.

**Do:** make Bevy's tracing profilers usable without giving up early
(pre-window) logging. Options:

- Add the Tracy / Chrome layers to **our** subscriber (behind the same feature
  flags), so `bevy/trace*` spans flow into a layer we control. Needs
  `bevy/trace` (the spans) enabled and the `tracing-tracy` / `tracing-chrome`
  layers wired into `init_tracing`, ideally as the `profile-tracy` /
  `profile-tracy-memory` viewer features [[viewer-profiling]] already scopes.
- Or restructure so `LogPlugin` owns logging (build the `App` first / let it
  buffer the early login logs), regaining the built-in profilers for free.

Until then, ad-hoc profiling uses `RenderDiagnosticsPlugin` +
`LogDiagnosticsPlugin` and bespoke `SL_VIEWER_DIAG` timers (a main-world
Update-schedule timer, a `FaceMaterial` modified-event counter) — enough to
split CPU vs GPU and main-world vs render-world, but not per-system.

## Done (2026-07-28)

Took the first option: the Tracy / Chrome layers are added to **our**
subscriber. `init_tracing` (`sl-client-bevy-viewer/src/lib.rs`) now re-creates
the layers `LogPlugin` would have, behind three viewer Cargo features:

- `profile-tracy` → `dep:tracing-tracy` + `bevy/trace` + `bevy/trace_tracy`
  (the last also makes `bevy_render` emit the per-frame `tracy.frame_mark`
  event our `TracyLayer` consumes; that event is filtered out of the fmt log).
- `profile-tracy-memory` → `profile-tracy` + `bevy/trace_tracy_memory` (Bevy's
  `bevy_log` provides the process-wide profiled `#[global_allocator]`, so no
  allocator wiring is needed on our side).
- `profile-chrome` → `dep:tracing-chrome` + `bevy/trace` + `bevy/trace_chrome`;
  honours `TRACE_CHROME`, names spans by formatted fields, and its `FlushGuard`
  is returned in a new `TracingGuards` value each `run()` holds for the process
  lifetime so the trace file finalises.

`tracing-tracy`/`tracing-chrome` are pinned to Bevy 0.19 `bevy_log`'s versions
(`0.11.4` / `0.7.0`) so the transitive `tracy-client` unifies on one build
(verified: single `tracy-client 0.18.4`). Documented in the book
(`book/src/tools/profiling.md`). The broader [[viewer-profiling]] deliverables
(RenderDiagnosticsPlugin rows in the statistics floater, dhat heap-regression
tests, samply notes, assert-no-alloc) remain open there.
