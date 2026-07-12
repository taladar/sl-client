---
id: viewer-p24-1
title: Sun / moon shadow maps
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 24 — Shadows
---

Context: [context/viewer.md](../context/viewer.md).

**P24.1. Sun / moon shadow maps.** Enable Bevy cascaded shadow maps on
the directional light, driven by the P22.2 sky sun direction, with cascades
tuned to region scale. Reference: `LLPipeline::renderShadow` /
`RenderShadowDetail`. Done: `sky::setup_sky` enables `shadow_maps_enabled` on
the `SceneSun` `DirectionalLight` and attaches a four-cascade
`CascadeShadowConfig` reaching a region diagonal (~384 m) with a tight near
cascade; `main` raises `DirectionalLightShadowMap` to 4096 for region-scale
texel density. Prims and avatars (`StandardMaterial`) cast/receive out of the
box, but the ground — the primary receiver — is the custom `TerrainMaterial`,
so `terrain.wgsl` was reworked to read the shared view + light bind group:
it now takes the sun/moon direction from the scene's first directional light
(so the ground also tracks the day cycle, superseding its old hard-coded sun)
and samples the cascaded shadow maps via `shadows::fetch_directional_shadow`,
multiplying only the direct term by the shadow factor.
