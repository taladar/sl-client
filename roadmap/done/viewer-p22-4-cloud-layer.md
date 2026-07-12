---
id: viewer-p22-4
title: Cloud layer
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 22 — Sky & atmosphere (day cycle, EEP)
---

Context: [context/viewer.md](../context/viewer.md).

**P22.4. Cloud layer.** Render the scrolling cloud layer — port
`cloudsV/cloudsF.glsl` / `LLVOClouds` with the sky frame's
`cloud_pos_density1/2`, `cloud_scale`, `cloud_scroll_rate`, `cloud_shadow`,
`cloud_variance` and `cloud_color`, sampling the (boosted) `cloud_texture`.

**Done.** New `CloudMaterial` / `clouds.wgsl` in `sl-client-bevy` (like
`SkyMaterial`, `bevy_pbr`-gated) porting `cloudsV.glsl` + `cloudsF.glsl`. The
reference computes the cloud lighting *per vertex* (`cloudsV`) and samples the
multi-octave noise *per fragment* (`cloudsF`); this evaluates the whole thing
*per fragment* on a camera-centred inward dome — the same approach `sky.wgsl`
takes for the sky — so the clouds are smooth without a dense mesh. The cloud
texcoords come from the reference dome's planar UV (`((-z + 1) / 2,
(-x + 1) / 2)` of the view direction), here derived per fragment; the
atmospheric inputs (`blue_horizon` / `blue_density` / `haze_*` /
`density_multiplier` / `max_y` / `glow` / `sunlight_color` / `ambient_color` +
`lightnorm`) are the sky frame's, so the cloud lighting matches the dome. New
viewer systems in `sky.rs`: `setup_clouds` spawns the cloud dome (radius just
inside `SKY_DOME_RADIUS` so the alpha-blended layer depth-tests in front of
the opaque sky without z-fighting) + a `CloudMaterial`, `drive_clouds` folds
the active sky frame into the material, accumulates the cloud scroll (the
reference `LLEnvironment::updateCloudScroll`: `delta += dt *
cloud_scroll_rate / 100`, folded into `cloud_pos_density1` with the x offset
negated per `LLSettingsVOSky::applySpecial`), and requests the sky frame's
`cloud_texture` (or the built-in `DEFAULT_CLOUD_ID`) boosted at
`SKY_BOOST_PRIORITY`; `apply_cloud_textures` swaps the decoded noise in;
`center_sky_on_camera` now follows both domes. **Key fix:** the cloud shader
tiles the noise (`cloud_scale` magnifies the UVs and the `cloud_pos_density` /
scroll offsets push them well outside `[0, 1]`), so the cloud image needs a
**repeating** sampler — Bevy's default clamp-to-edge otherwise smears the
black edge texel across the whole layer (noise sampled as `0` everywhere → no
clouds, only a thin projection-stretch artifact); giving the cloud image
`ImageAddressMode::Repeat` (as the prim/terrain textures already do, matching
the reference `GL_REPEAT`) makes the noise tile. Verified on OpenSim (pinned
mid-day): scattered white puffy clouds across the blue sky at the region's
default coverage, denser as `cloud_shadow` rises. The A/B day-cycle noise
blend is wired (`blend_factor`) but stays `0.0` until P22.6.
