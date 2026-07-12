---
id: viewer-p23-1
title: Water plane
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 23 — Water surface
---

Context: [context/viewer.md](../context/viewer.md).

**P23.1. Water plane.** Render a water plane at the region water height
with the EEP water settings (fresnel, reflection tint, scrolling wave
normals) — `LLVOWater` / `LLSettingsWater` + the water shaders — as a custom
Bevy material fed by P22.1's environment settings.

**Done (surface + underwater fog), verified live on OpenSim.** Three layers,
built to reproduce Firestorm as closely as the headless pipeline allows:

*`sl-proto` (Bevy-free):* `WaterSettings::blend` (the day-cycle frame
interpolation, the water counterpart of `SkySettings::blend` — lerps the
fresnel / blur / fog / refraction scalars, the fog colour, the normal
(wavelet) `Scale`, and the two wave directions; snaps name + normal /
transparent textures at the half-way point), plus
`EnvironmentSettings::active_water_settings(position)` /
`blended_water_settings(position)` (water has **no** altitude tracks — one
region-wide `water_track` — so unlike the sky they take only a day-cycle
position). New `lerp_scale` helper; 5 new unit tests. `cargo test -p
sl-proto` green.

*`sl-client-bevy`:* new `WaterMaterial` / `water.wgsl` (`bevy_pbr`-gated, like
the sky materials), a port of `class1/environment/waterV.glsl` +
`class3/environment/waterF.glsl`. Per fragment it builds the three scrolling
wave-normal texcoords (`waterV`'s sweeping displacement + `waveDir`/time
scroll), samples the (blended `bumpMap`/`bumpMap2`) normal maps, and runs the
reference `calculateFresnelFactors` (the `df3` three-term squared fresnel →
reflection amount `df2.x`, plus `df2.y`) and `color = mix(fb, radiance,
df2.x) + punctual`. The two G-buffer-dependent inputs the headless pipeline
lacks are substituted by the reference's own fallbacks: `fb` (screen
refraction) → the **water-fog colour** (exactly `applyWaterFogViewLinear` over
white, the non-transparent-water path), and `radiance` (reflection probe) →
a **sky reflection tint**; a Blinn-Phong sun glint stands in for the
`pbrPunctual` specular.
The per-wave fresnel dot is taken as `-abs(dot(view, wave))` so the surface
shades as water from **both** faces (an underwater camera looking up at the
underside reads as water, not a grazing sky reflection). Re-exported
`WaterMaterial` / `WaterParams` / `WaterMaterialPlugin`.

*Viewer `water.rs`:* per the reference `LLDrawPoolWater::render`, the water
**colour / waves / fresnel are region-wide** (a single `getCurrentWater()` —
the position-selected current EEP water — binds the whole water pass), so one
**shared** material drives every plane; only the water **height** varies per
region. `setup_water` spawns the **endless ocean** (a large camera-following
plane at the agent region's water height — the reference hole/edge water that
fills the sea wherever there is no loaded region); `drive_water` learns each
region's water height from its `RegionInfoHandshake` and spawns a **per-region
plane** for any neighbour whose height *differs* from the agent region's (a
region with a different sea level renders at its own height; same-height
regions are covered by the ocean, so the common case is one clean surface).
Folds the blended EEP water settings + sun direction + a sky reflection tint +
wave-scroll time into the shared material each frame and fetches the wave
normal map (`DEFAULT_WATER_NORMAL` or the frame's own) boosted.

*Viewer `underwater_fog.rs`:* a **fullscreen post-process** reproducing the
reference water fog (`getWaterFogViewNoClip` / `applyWaterFogViewLinear`) over
the *whole* scene — a per-material fog would miss objects / avatars, so this
runs once on the composited image + the scene depth, fogging terrain, objects,
avatars, and the water underside uniformly. It reconstructs each pixel's world
position from depth and applies the reference's **per-fragment water-plane
clip** (a fragment above the surface passes through untouched), so a camera
straddling the surface splits cleanly along the waterline and underwater
objects seen from above water still fog. `waterFogKS = 1 / max(lightDir.z,
0.3)` and `getModifiedWaterFogDensity` (`pow(density, fogMod)` when the eye is
submerged) are reproduced. The fog is applied after the main pass (display
space, a pragmatic deviation from the reference's linear deferred stage; the
distance falloff / clip are the reference's).

**Bevy 0.19 render-API note (cross-cutting).** Bevy **0.19 replaced the render
graph** with a system-based renderer (passes are systems in the `Core3d`
schedule; `RenderContext` is a system param; pipelines specialize by the
view's `target_format`; the `FullscreenMaterial` trait exists but its bind
group is fixed to *(source, sampler, uniform)* with no depth binding). The fog
is therefore a hand-written render system modelled on
`bevy_post_process::effect_stack`. Depth comes from the **main-pass depth
texture** made sampleable via `Camera3d::depth_texture_usages |=
TEXTURE_BINDING` — **not** a `DepthPrepass`, because the prepass builds depth
pipelines for the custom sky / terrain / water materials whose `specialize`
pins bespoke vertex layouts that the prepass vertex shader rejects (a
validation error); the main depth already carries every material's depth with
no extra pipelines. The camera pins `Msaa::Sample4` so that depth texture is
multisampled to match the fog's `texture_depth_2d_multisampled` binding. The
three Bevy migration guides (0.16→0.17→0.18→0.19) are now referenced in the
sl-client skill. **Deferred:** transparent-water refraction (seabed sharply
through the surface) needs a screen-copy the headless viewer lacks; the
clouds' vertical-orientation bug noticed here is tracked as **R18**.
