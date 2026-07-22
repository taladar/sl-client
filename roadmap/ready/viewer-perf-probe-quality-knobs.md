---
id: viewer-perf-probe-quality-knobs
title: Reflection-probe quality settings (detail, resolution, pool, budget)
topic: viewer
status: ready
origin: reflection-probe performance planning round (2026-07-22), Firestorm settings survey
refs: [viewer-perf-probe-scheduling, viewer-perf-probe-capture-content, viewer-graphics-presets, viewer-preferences-graphics-tab, viewer-ui-settings-binding]
---

Context: [context/viewer.md](../context/viewer.md).

All probe tuning is compile-time constants (`CAPTURE_SIZE = 128`,
`MAX_LOCAL_PROBES = 4`, `CAPTURE_PERIOD_FRAMES = 180` in `probes.rs`)
plus a couple of debug env vars; the only way to turn probes off today
is a recompile. In the reference the user runs with probes **off 99% of
the time** — that mode must be first-class here, not a workaround.

These are **preferences settings**, not CLI options (GUI-driven
persistent graphics settings belong in the settings store; the CLI is
for non-GUI or needed-at-startup settings): named entries in the typed
settings store, so they persist per scope, bind to widgets through
[[viewer-ui-settings-binding]], and
surface on the [[viewer-preferences-graphics-tab]] (whose scope already
lists reflection probes) — with [[viewer-graphics-presets]] later
saving/loading the whole group. This task defines the settings and their
runtime plumbing; the tab supplies the UI.

Settings, modelled on the reference's tiers:

- **Probe detail** — off / static / dynamic / realtime
  (`RenderReflectionProbeDetail` analogue). `off` despawns/skips the
  rigs entirely **and restores the non-probe lighting model**: keep the
  flat sky `GlobalAmbientLight` (bypass `suppress_global_ambient`) and
  the terrain / water shaders' no-probe fallbacks — the world must stay
  lit. `static` freezes captures after each rig's first fill; `dynamic`
  honours the DYNAMIC flag ([[viewer-perf-probe-capture-content]]);
  `realtime` additionally lets one probe run at burst-per-frame cadence.
- **Probe resolution** — 64–512, power of two (the
  `GeneratedEnvironmentMapLight` filter's constraint).
- **Local probe count** — the pool size (0 = default probe only, the
  reference's "one probe to rule them all" level).
- **Frame-time budget target** for the capture scheduler, once
  [[viewer-perf-probe-scheduling]] lands, rather than a raw period.

Mechanically: the constants become a `ProbeSettings` resource kept in
sync from the settings store; the probe systems read it. Because
settings change at runtime (unlike a launch flag), define live-apply
semantics: detail and budget apply immediately; resolution and pool
size rebuild the capture rigs on change (rigs are currently built once
at startup — teach setup to tear down and respawn, reusing the
existing rig-reassignment machinery, rather than documenting a
restart requirement).

Acceptance: detail `off` restores pre-P33 baseline FPS and a lit (not
black) world in `render_gallery`; resolution 256 gives a sharper
mirror-sphere golden; pool 0 leaves only the default probe live
([[viewer-perf-probe-instrumentation]] counters confirm); flipping each
setting at runtime takes effect without relogin.
