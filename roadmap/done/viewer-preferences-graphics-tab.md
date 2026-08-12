---
id: viewer-preferences-graphics-tab
title: Preferences — graphics tab
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-preferences-floater]
refs: [viewer-quick-preferences]
---

Context: [context/viewer.md](../context/viewer.md).

The **graphics** tab of the preferences floater
([[viewer-preferences-floater]]): draw distance, render-quality tiers, shadows,
reflection probes, the tone mapper / exposure, ambient occlusion, anti-aliasing,
avatar-complexity / imposter limits, and the other render knobs — each control
bound to the typed settings store through the floater's binding.

Render effects ship enabled-by-default and env-gated; this tab surfaces the
settings that drive them, it is not a build prerequisite for any effect.
Named save/load of this whole settings group is
[[viewer-graphics-presets]], which builds on this tab.

Quick Preferences: several of these are reached-for-hourly and belong in the
Quick Preferences panel too ([[viewer-quick-preferences]]) — the render-quality
tier and `RenderVolumeLODFactor` especially (draw distance is already there).
When such a setting is introduced here, add a default entry for it in
`default_entries()` (`quick_preferences.rs`) plus a Fluent label; the panel is a
view over the same store and binds by setting key, so nothing is reimplemented.

Reference (Firestorm, read-only): `llfloaterpreference*` (the graphics panels).

Builds on: [[viewer-preferences-floater]].

## Done

New viewer module **`src/preferences_graphics.rs`**
(`PreferencesGraphicsPlugin`), the second `PREF_TABS` entry ("Graphics"):

- **Surfaced existing live settings** (each already applied by its
  feature module): draw distance, `RenderVolumeLODFactor`,
  `RenderMaxPartCount`, the four glow settings, `RenderTonemapType` /
  `RenderTonemapMix` / `RenderExposure`,
  `RenderDynamicExposureEnabled` plus
  `RenderSkyAutoAdjustLegacy` (the coefficient / speed / use-sky
  knobs stay debug-only), probe dynamic content, `render_mirrors`,
  `render_hero_probe_resolution` (restart-scoped — the row label says
  so) and `render_hero_probe_update_rate`.
- **New settings with new appliers**: `RenderShadowDetail` (0 none /
  1 sun-moon, flips the scene sun's `shadow_maps_enabled`),
  `RenderShadowMapSize` (live `DirectionalLightShadowMap` writes,
  clamped power-of-two 1024–8192), `RenderShadowCascades` (rebuilds
  the sun's `CascadeShadowConfig` via `shadow_cascades_for`),
  `RenderVSyncEnable` (primary-window present mode; the startup `Fifo`
  counts as vsync-on so launch never reconfigures the surface),
  `FSLimitFramerate` + `FramePerSecondLimit` (a `Last`-schedule frame
  sleep, the bevy_framepace mechanism). The `SL_VIEWER_SUN_SHADOWS` /
  `SL_VIEWER_SHADOW_CASCADES` harness envs win over the stored values.
- **Quality tier** (`RenderQualityPerformance`, 7 tiers Low–Ultra): a
  driver combo writing the `QUALITY_TIERS` row (far clip, LOD factor,
  particles, shadow detail + map size, glow, probe dynamic content,
  mirrors) through the store on a **user pick only** — programmatic
  combo writes emit no `ComboChanged`, so the shell's Cancel snapshot
  revert never re-triggers a tier. Tiers deliberately skip tone
  mapping / exposure (aesthetic), mirror resolution (restart-scoped),
  vsync / FPS cap (user policy) and cascades. Manual edits of member
  settings do not move the stored tier (reference behaviour).
- Quick Preferences gained the LOD-factor slider and the
  quality-preset combo (same `QualityTierControl` marker, one applier
  serves both surfaces). Note: an existing per-avatar
  `quick_preferences.json` keeps its own entry list — restore defaults
  (or hand-edit) to see the new rows.
- Verified by 8 headless tests (tier monotonicity + option-count pin,
  registered defaults, frame-sleep budget cases, shadow detail / map
  size / vsync appliers, tier-applies-on-user-pick-only incl. the
  revert-safety case) and live on the local grid: layout screenshot on
  the graphics tab (combo defaults ACES / 4096 / Sun-and-moon / 512),
  shadow toggle (vegetation shading vanishes at detail 0), FPS cap
  (status bar 31 fps at a 30 cap; 60 uncapped).

Deliberately **not** in this tab (no consumer exists yet):
anti-aliasing (MSAA is pinned 4× to match the underwater-fog depth
binding), ambient occlusion / SSR (no such passes), avatar complexity
/ impostor limits ([[viewer-avatar-complexity-limit]] /
[[viewer-avatar-impostors-billboard]] own those controls), VRAM budget
([[viewer-texture-vram-budget]]). Named save / load of the group is
[[viewer-graphics-presets]], which this unblocks.
