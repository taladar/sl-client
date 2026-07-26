---
id: viewer-profiling-logplugin-tracing
title: Custom tracing subscriber blocks Bevy's trace_tracy / trace_chrome profilers
topic: viewer
status: ideas
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
