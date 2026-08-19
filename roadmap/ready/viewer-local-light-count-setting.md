---
id: viewer-local-light-count-setting
title: Local-light budget & lighting-detail settings
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-p25-2, viewer-projector-lights-textured,
  viewer-perf-probe-quality-knobs, viewer-preferences-graphics-tab]
---

Context: [context/viewer.md](../context/viewer.md).

The reference renders up to `RenderLocalLightCount` local lights per frame
(default 256, clustered), and Firestorm adds `RenderShaderLightingMaxLevel`
— the graphics-tab "local lights detail" combo choosing between local
lights off, nearby lights only, and all lights. Together they are the main
user lever for lighting cost: turn point lights off entirely in a laggy
club, or raise the budget on a strong GPU.

Our local-light renderer ([[viewer-p25-2]]) picks the nearest / brightest
light prims into a fixed compile-time budget: `MAX_LOCAL_LIGHTS = 32` in
`sl-client-bevy-viewer/src/lights.rs` — well under the reference's 256,
with no runtime setting at all. There is no off switch (no way to fall
back to sun/moon-only lighting) and no way to raise the cap. Bevy's
clustered forward path can handle far more than 32 lights; mind the
documented ClusterZConfig::FixedZ gotcha (the default Bevy Z-slicing
drops near lights) when raising the budget.

Scope: a `[render]` settings pair — a local-light count (0 disables local
lights entirely) and optionally a detail tier matching the Firestorm
combo — consumed live by `render_local_lights`, registered like the
existing shadow/glow settings so the graphics tab and the
`RenderQualityPerformance` quality tiers (`preferences_graphics.rs`,
`QUALITY_TIERS`) can bind them; each tier should gain a light-budget
ramp. Projector lights ([[viewer-projector-lights-textured]]) draw from
the same budget once they land.

Reference (Firestorm, read-only):
`indra/newview/app_settings/settings.xml` (RenderLocalLightCount),
`indra/newview/skins/default/xui/en/panel_preferences_graphics1.xml`
(RenderShaderLightingMaxLevel "LocalLightsDetail" combo).
