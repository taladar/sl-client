---
id: viewer-performance-floater
title: Performance floater — "Improve graphics speed…" + auto-FPS tuner
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-avatar-complexity-limit, viewer-statistics-floater,
  viewer-avatar-render-settings-manager, viewer-graphics-presets,
  viewer-name-tags-complexity-distance]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's performance floater — World ▸ "Improve graphics speed…"
and Advanced ▸ Performance Tools open the same `performance` floater
(LL `llfloaterperformance` over the `llperfstats` sampling layer;
Firestorm additionally ships its own `fsfloaterperformance` variant) —
is a one-stop "why is my FPS low" triage hub. It has an overview page
with the current frame rate and the top frame-cost drivers, a
nearby-avatars pane ranked by per-avatar render cost with one-click
per-avatar actions (always render / never render, i.e. the per-avatar
render-settings verbs), a worn-HUDs pane with per-HUD complexity cost
and totals, quick shortcuts to the handful of heaviest graphics
settings, and the auto-FPS tuner: `AutoTuneFPS` / `AutoTuneLock` (with
a `TuningFPSStrategy`) makes the viewer walk quality settings up or
down toward a user-set target frame rate, optionally only while the
window has focus. Its toolbar command (`commands.xml` command
`performance`) carries the auto-tune enable as the button's checkbox
(`checkbox_control="AutoTuneFPS"`).

We already have the ingredients the floater reads: avatar-complexity
accounting and limiting ([[viewer-avatar-complexity-limit]], done),
per-frame timings and the pipeline-status API, and the graphics
preferences tab / presets ([[viewer-graphics-presets]]) — but no
floater that aggregates them and no auto-tuner. Scope: the floater
shell with the overview, ranked nearby-avatar and HUD cost lists fed
from our complexity data, per-avatar one-click actions reusing (not
duplicating) [[viewer-avatar-render-settings-manager]], the quick
graphics-lever controls, and the closed-loop "adjust settings until
target FPS" controller over our graphics settings store. The stats
detail overlaps [[viewer-statistics-floater]]; our engine differs
(Bevy, own perf roadmap), so the tuner's setting-ladder must be
defined against our own cost knobs rather than copied verbatim.

Reference (Firestorm, read-only): `indra/newview/llfloaterperformance.cpp`,
`indra/newview/llperfstats.cpp`, `indra/newview/fsfloaterperformance.cpp`,
`indra/newview/skins/default/xui/en/floater_performance.xml`,
`indra/newview/skins/default/xui/en/floater_fs_performance.xml`,
`indra/newview/app_settings/commands.xml` (command `performance`),
`menu_viewer.xml` L1285 / L3172.
