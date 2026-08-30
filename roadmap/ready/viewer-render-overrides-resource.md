---
id: viewer-render-overrides-resource
title: Render A/B knobs become a per-app resource, not process-global env locks
topic: viewer
status: ready
origin: test-harness plan (2026-08-30)
points: 3
refs: [viewer-render-type-toggles]
---

Context: [context/testing.md](../context/testing.md).

Every render A/B knob is a process-global `OnceLock` read from the
environment: `glow_overrides`, `exposure_overrides`, `tonemap_overrides`,
`fog_disabled`, `pre_water_pass_disabled`, the HUD-particles memo and
`pinned_day_position` (nine call sites). Two apps in one test binary
cannot disagree, so the toggle axis of the render matrix cannot exist.

Add `RenderOverrides` (`sl-viewer-world-scene/src/render_overrides.rs`):
a `Resource` whose `Default` overrides nothing, `from_env()` reads every
`SL_VIEWER_*` knob once in `run_viewer` (names and semantics unchanged),
and `set(toggle, on)` flips one for a test. Consumers switch to
`Res<RenderOverrides>` (an `ExtractResourcePlugin` where a reader is
render-side); the day-position pin moves onto `EnvironmentState` as
`day_position()`. Logging-only `SL_VIEWER_LOG_*` locks are out of scope.

Acceptance: a unit test builds two apps with opposite overrides in one
process; the live viewer behaves identically under each env var.
