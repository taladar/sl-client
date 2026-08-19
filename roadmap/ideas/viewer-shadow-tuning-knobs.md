---
id: viewer-shadow-tuning-knobs
title: Advanced shadow tuning (bias / blur / softness)
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-p24-1, viewer-projector-lights-spot-shadows,
  viewer-phototools]
---

Context: [context/viewer.md](../context/viewer.md).

Phototools exposes the deferred shadow tuning set — `RenderShadowBias`,
`RenderShadowBlurSize`, `RenderShadowBlurDistFactor`,
`RenderShadowFOVCutoff`, `RenderSpotShadowOffset` — because photographers
tune shadow softness and fight acne / peter-panning per shot.

Our sun/moon CSM ([[viewer-p24-1]]) uses Bevy's defaults (depth/normal
bias, PCF) with only the detail / map-size / cascade-count settings in
`sl-client-bevy-viewer/src/preferences_graphics.rs`; nothing lets a user
trade acne against peter-panning or soften shadow edges.

Scope: surface the Bevy equivalents as live `[render]` settings — the
directional light's `shadow_depth_bias` / `shadow_normal_bias`, and a
soft-shadow kernel size where the pipeline allows — and map what has no
Bevy analogue honestly (document the divergence rather than fake a knob).
Projector (spot) shadow offsets join when
[[viewer-projector-lights-spot-shadows]] lands. Surfacing belongs on
[[viewer-phototools]]'s Light and Shadows tab.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_phototools.xml` (Light and
Shadows tab), `indra/newview/pipeline.cpp` shadow pass.
