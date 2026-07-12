---
id: viewer-p22-5
title: Stars
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 22 — Sky & atmosphere (day cycle, EEP)
---

Context: [context/viewer.md](../context/viewer.md).

**P22.5. Stars.** Render the star field at night (the reference star
pass / `star_brightness`), fading in as the sun sets.

**Done.** New `StarMaterial` / `stars.wgsl` in `sl-client-bevy` (like
`SunDiscMaterial`, `bevy_pbr`-gated) porting `class1/deferred/starsV.glsl` +
`starsF.glsl` (`LLDrawPoolWLSky::renderStarsDeferred` /
`LLVOWLSky::drawStars`). Unlike the sky / cloud domes (one inward sphere
evaluated per fragment), the star field is **real quad geometry** — the
viewer builds a mesh of 1000 star quads (the reference `getStarsNumVerts`),
each a small camera-facing square with a per-star near-white colour, sampled
from the sky's **bloom** texture (`IMG_BLOOM1`, the reference's star sprite —
`getBloomTex`, boosted at `SKY_BOOST_PRIORITY`) and drawn **additively**
(`AlphaMode::Add` = the reference `BT_ADD_WITH_ALPHA`) so the black bloom
texels add nothing and only the bright star texels light the sky. The
per-fragment `twinkle()` (a sawtooth of the model-space position scaled by
`time`) and the `custom_alpha = star_brightness / 500` fade are the
reference's; the field is hidden below the reference `0.001` threshold, so it
fades in exactly as `star_brightness` rises through the day-cycle keyframes
(smooth blend is P22.6). New viewer systems in `sky.rs`: `setup_stars` builds
the deterministic (fixed-seed SplitMix64, standing in for `ll_frand`) star
mesh; `drive_stars` centres the field on the camera, spins it very slowly
about the up axis (the reference `rotatef(gFrameTimeSeconds * 0.01, …)` — in
**degrees**, converted to radians), folds `star_brightness` / twinkle time
into the material, and requests the bloom texture boosted;
`apply_star_textures` swaps the decoded bloom in. **Star size:** the reference
sizes its quads (`sc = 16 + frand * 20`) for its 15000 m `DOME_RADIUS`; ours
sit at a nearer radius for screen projection, so the per-star size is scaled
by `radius / 15000` to keep the same *angular* size (otherwise ~5× too big).

**Far-plane skybox rework (cross-cutting, revisits P22.2–P22.4).** Stars
exposed a latent depth limitation: the P22.2 sky dome was an **opaque
world-space sphere at 3000 m that wrote depth**, so anything past ~3000 m from
the camera (a 2000 m skybox, a tall build — content SL routinely has up to
4096 m) was occluded by it, and stars had to sit inside it. Fixed by turning
the sky, cloud, and star domes into a proper **skybox backdrop**: each vertex
shader now forces its fragment to the reverse-Z far clip plane
(`clip_position.z = 0`). Bevy's mesh pipeline uses a `GreaterEqual` depth
test, so `0 >= 0` still draws the backdrop over the cleared (far) background
while `0 >= any nearer geometry` fails — real scene geometry at **any**
altitude now occludes the sky/clouds/stars, and the domes never hide objects
beyond their own radius. The sun / moon discs deliberately keep their real
2000 m world-space depth, so a disc still draws in front of the far-plane star
field (occluding the stars behind it) — the reference's "moon writes depth to
clip stars" intent. Verified on OpenSim (pinned night: pinpoint stars, moon,
clouds, the own avatar correctly occluding the stars behind it; pinned midday:
intact blue haze-graded sky, clouds, terrain, no stars).
