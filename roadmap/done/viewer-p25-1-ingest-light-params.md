---
id: viewer-p25-1
title: Ingest light params
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 25 — Local lights
---

Context: [context/viewer.md](../context/viewer.md).

**P25.1. Ingest light params.** Read a prim's light block (colour,
radius, falloff, intensity, and spot cone params) from its light
extra-params (`LLLightParams`). Done: a new viewer `lights` module decodes
`object.extra.light` (+ the companion `light_image` when the prim is a
spotlight/projector) into an `ObjectLight` component — linear RGB + intensity
(the wire colour alpha, per `LLVOVolume::getLightIntensity`), radius, falloff,
cutoff, and an optional `LightProjection` (texture + fov/focus/ambiance).
`apply_object` inserts / refreshes / removes the component on every object
update; the crosshair pick (`P`) reports the decoded light, and a `debug!`
logs each ingest. Wire colour bytes are the **linear** colour (Firestorm's
`LLLightParams::unpack` feeds them straight to `setLinearColor`), so no sRGB
decode. Verified live on OpenSim against a provisioned orange point-light prim
(`emitted=[0.8,0.398,0.0]`, i.e. linear `[1,0.5,0]` scaled by intensity `0.8`,
radius 10 m, falloff 1). P25.2 will read `ObjectLight` to spawn Bevy lights.
