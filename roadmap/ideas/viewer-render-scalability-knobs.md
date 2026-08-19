---
id: viewer-render-scalability-knobs
title: Leftover per-family render scalability knobs
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-p2-1, viewer-p23-1, viewer-p34-1, viewer-p34-2,
  viewer-phototools, viewer-mesh-lod-factor-preference,
  viewer-preferences-graphics-tab]
---

Context: [context/viewer.md](../context/viewer.md).

A cluster of small reference scalability levers whose render feature has
no home task here. Each is cheap individually; none justifies its own
task. The value is mostly perf headroom on weak machines plus
[[viewer-phototools]] parity:

- Terrain geometry LOD (`RenderTerrainLODFactor`): our `terrain.rs`
  renders every heightfield patch at full resolution; distant-patch
  decimation is the reference's terrain cost lever.
- Opaque-water fallback (`RenderTransparentWater` off): render cheap
  opaque water instead of the full plane — our `water.rs` is always the
  transparent/refractive path.
- Dynamic LOD (`RenderDynamicLOD`): re-evaluate object LOD continuously
  versus only when the camera stops.
- Far-clip disable (`RenderUseFarClip`, phototools): draw everything
  regardless of distance for a panorama shot.
- Avatar-physics LOD (`RenderAvatarPhysicsLODFactor`): degrade the
  physics-wearable spring sim with distance; our `body_physics.rs` runs
  full-rate for every avatar.
- A separate tree LOD factor (`RenderTreeLODFactor`): today tree geometry
  rides the unified LOD factor in `render_priority.rs`
  ([[viewer-mesh-lod-factor-preference]]); a per-family factor is a
  pref-granularity nicety.

The graphics-preferences audit also flagged the remaining graphics-tab
controls with no preference surface yet; fold their dispositions in here
rather than an umbrella prefs task. Needing only a settings row over an
existing constant or capability: `RenderGlowResolutionPow` (glow buffer
size is the `GLOW_RESOLUTION = 512` constant in `glow.rs`),
`RenderTerrainScale` (terrain detail-repeat span constant,
`terrain.rs`), `RenderNormalMapScale` (bump height-scale constant,
`bump.rs`), and `AgentPause` (freeze world — the protocol command exists
both directions in sl-proto, no UI). Owned elsewhere:
`RenderLocalLightCount` / `RenderShaderLightingMaxLevel` →
[[viewer-local-light-count-setting]], `RenderAutoMaskAlphaDeferred` →
[[viewer-alpha-auto-mask]], `RenderFlexTimeFactor` →
[[viewer-perf-flexi-distance-lod]], `RenderCompressTextures` →
[[viewer-perf-texture-decode-cache]], `RenderAvatarLODFactor` →
[[viewer-perf-avatar-mesh-lod-screen-size]],
`RenderMaxTextureResolution` / `TextureDiscardLevel` →
[[viewer-texture-vram-budget]]. Consciously nothing-to-do (record only):
`WLSkyDetail` (our atmospherics are per-pixel, dome tessellation is
moot), `RenderDisableVintageMode` (no LDR fallback exists),
`RenderUnloadedAvatar` (we never gate avatars behind a cloud),
`PrecachingDelay` (obsolete login-delay timer).

Reference (Firestorm, read-only):
`indra/newview/app_settings/settings.xml` (each key),
`indra/newview/lldrawpoolwater.cpp`,
`indra/newview/llvosurfacepatch.cpp`,
`indra/newview/llphysicsmotion.cpp` (RenderAvatarPhysicsLODFactor),
`indra/newview/skins/default/xui/en/panel_preferences_graphics1.xml`,
`indra/newview/skins/default/xui/en/floater_preferences_graphics_advanced.xml`.
