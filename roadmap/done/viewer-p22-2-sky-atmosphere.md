---
id: viewer-p22-2
title: Sky & atmosphere
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 22 — Sky & atmosphere (day cycle, EEP)
---

Context: [context/viewer.md](../context/viewer.md).

**P22.2. Sky & atmosphere.** Render the atmospheric sky dome — port the
Rayleigh / Mie scattering of `LLVOSky` / `LLVOWLSky` (+ the `skyV` / `skyF`
deferred shaders) into a Bevy sky material; drive the sun / moon direction
and colours, and set the scene directional light + ambient, from the sky.
Select the active `sky_frames` entry by the camera's altitude against
`EnvironmentSettings::track_altitudes` (region = default, parcel = override).
Any sky / sun / moon / cloud / bloom / halo / rainbow texture the sky frame
references must be fetched **boosted** through the texture manager
(`request_boosted`, a new `SKY_BOOST_PRIORITY` mirroring `LLGLTexture::
BOOST_HIGH`) so it resolves ahead of ordinary scene faces, exactly like the
terrain / avatar textures.

**Done (dome + lighting core; sun/moon disc, clouds and stars split out to
P22.3–P22.5 below).** New `SkyMaterial` / `sky.wgsl` in `sl-client-bevy` (like
`TerrainMaterial`, `bevy_pbr`-gated) transcribing the reference
`class1/deferred/skyV.glsl` + `skyF.glsl` — the legacy two-colour exponential
atmosphere (`blue_horizon` / `blue_density` / `haze_*` / `density_multiplier`
/ `max_y` / `glow` scattering with the anti-solar glow) plus the rainbow /
halo overlays. The reference computes the haze colour *per vertex* on a
tessellated dome; this evaluates the identical math *per fragment* on a
camera-centred inward-facing sphere, so the sky is smooth without a dense
mesh. New viewer `sky.rs`: `setup_sky` spawns the dome + the scene's sun/moon
directional light; `center_sky_on_camera` keeps the dome on the camera;
`drive_sky` selects the active `SkySettings` for the camera altitude (the
reference `calculateSkyTrackForAltitude`, added Bevy-free as
`EnvironmentSettings::sky_track_for_altitude` / `active_sky_settings`),
computes the sun/moon direction + the scene light + ambient the way
`LLSettingsSky::calculateLightSettings` does, and folds them into the
material, the `DirectionalLight`, and the `GlobalAmbientLight`;
`apply_sky_textures` swaps each decoded overlay in. `request_boosted` already
existed from the P20 boost work, so the net-new was the `SKY_BOOST_PRIORITY`
band (above the avatar boost) used for the rainbow / halo maps. Re-exported
`Color` / `ColorAlpha` / `Glow` from both runtimes for parity. **Frame
selection is time-*active*, not altitude-only:** the roadmap says altitude,
but a single altitude track carries many day keyframes, so the active keyframe
is picked at the current region day-position (`fmod(now + day_offset,
day_length) / day_length`) *without* blending — the smooth keyframe
interpolation is P22.6. Debug affordance `SL_VIEWER_SKY_DAY_POSITION` pins the
day-position (0..1) so the offline screenshot harness can inspect any point in
the day (verified midday on OpenSim: a blue dome, paler at the horizon from
haze).
