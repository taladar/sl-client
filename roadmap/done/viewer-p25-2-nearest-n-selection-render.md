---
id: viewer-p25-2
title: Nearest-N selection + render
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 25 — Local lights
---

Context: [context/viewer.md](../context/viewer.md).

**P25.2. Nearest-N selection + render.** Spawn Bevy `PointLight` /
`SpotLight` for light-flagged prims, selecting the nearest / brightest N per
frame within a budget (GPU / clustered-light limits). Reference:
`LLPipeline::setupHWLights`, `LL_NUM_LIGHT_UNITS`. Done: `drive_local_lights`
reads each frame's `ObjectLight` components (P25.1), ranks them by emitted
luminance attenuated by camera distance (nearest / brightest first, mirroring
`setupHWLights` keeping only the closest lights), and spawns the top
`MAX_LOCAL_LIGHTS` (32) as Bevy lights — a `PointLight` for a plain light, a
`SpotLight` (cone from the projector's half-FOV, inner cone from its focus)
for a projector. Each Bevy light is a child of the light-flagged object
entity with an identity local transform, so it rides the prim's transform
and — for a
spotlight — its forward (`-Z`) already equals Second Life's spot direction
(`at_axis(0,0,-1) * render_rotation`) once the parent's coordinate conversion
is applied, needing no extra rotation. The SL colour carries the light hue and
the wire-alpha intensity rides Bevy's photometric lumens
(`LOCAL_LIGHT_LUMENS = 1_000_000`, Bevy's `VERY_LARGE_CINEMA_LIGHT`), so
radiance stays proportional to the emitted colour; the radius maps to the Bevy
light `range`. Each Bevy light child is **kept alive and updated in place**
across frames (tracked in a `LocalLights` object→child map, which also caches
the last-applied light so an unchanged prim does no per-frame ECS mutation); a
prim only gains a child on entering the budget and loses it on dropping out.
Re-spawning (or even re-inserting the light component on) the child every
frame churns the retained render world and makes the light *flicker* on lit
surfaces — the reconcile-in-place-on-change path is what fixes that (verified
live). A
change in the rendered count logs once at `debug`. SL `falloff` has no Bevy
analogue (Bevy's
point light uses a fixed smooth range attenuation) and the projected light
*texture* (`SpotLightTexture` / `PointLightTexture`) is not yet wired through
the texture pipeline — both are follow-ups. Verified live on OpenSim: the
provisioned orange point-light prim is selected (`rendering 1 of 1 candidate`)
and rendered without regressing the scene.
