---
id: viewer-p22-3
title: Sun & moon disc
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 22 — Sky & atmosphere (day cycle, EEP)
---

Context: [context/viewer.md](../context/viewer.md).

**P22.3. Sun & moon disc.** Render the sun and moon as textured billboards
at their computed directions (the reference `sunDiscV/F.glsl` /
`LLDrawPoolWLSky::renderHeavenlyBodies`), blended between the sky frame's two
sun textures. Fetch the sky frame's `sun_texture` / `moon_texture` (or the
reference defaults) **boosted** through the texture manager, as P22.2 already
does for rainbow / halo.

**Done.** New `SunDiscMaterial` / `sun_disc.wgsl` in `sl-client-bevy` (like
`SkyMaterial`, `bevy_pbr`-gated) porting `sunDiscV/F.glsl` + `moonV/F.glsl`.
It samples the disc texture (a `diffuse` / `alt_diffuse` pair blended by a
`blend_factor` left at `0.0` until the day cycle drives it in P22.6), applies
the moon's brightness, its transparent-pixel discard, and its near-horizon
alpha fade, and is drawn `AlphaMode::Blend` over the (opaque) dome. **The
reference does *not* tint the disc by its diffuse colour**: the CPU binds
`DIFFUSE_COLOR` (sun) / `color` (moon) but `sunDiscF` never declares it and
`moonF` declares yet never reads it, so both are dead uniforms; the disc
shows its texture as-is (moon only scaled by `moon_brightness`). New viewer
systems in `sky.rs`: `setup_sun_moon_discs` spawns two billboard quads (a
shared unit `Rectangle` + a `SunDiscMaterial` each); `drive_sun_moon_discs`
aims each disc at its Bevy-space direction (same `sky.{sun,moon}_rotation` as
`drive_sky`) as a camera-facing billboard (the reference `hb_right` / `hb_up`
basis + near-horizon enlargement, in `disc_transform`), sizes it by the
reference `HEAVENLY_BODY_FACTOR` × disk radius × `{sun,moon}_scale` at a fixed
`DISC_DISTANCE` (inside the dome so it depth-tests in front), shows only the
bodies above the horizon (`getIsSunUp` / `getIsMoonUp`), and requests the
`sun_texture` / `moon_texture` (or the built-in `DEFAULT_SUN_ID` /
`DEFAULT_MOON_ID`) boosted at `SKY_BOOST_PRIORITY`; `apply_disc_textures`
swaps each decoded disc in. Verified on OpenSim (pinned mid-day, camera aimed
up: a bright glowing sun disc haloed into the atmosphere; the moon likewise).
The A/B day-cycle texture blend is wired (`blend_factor`) but stays `0.0`
until P22.6 supplies a next-frame texture.
