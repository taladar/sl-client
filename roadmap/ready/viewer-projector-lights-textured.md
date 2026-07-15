---
id: viewer-projector-lights-textured
title: Projector lights (textured spotlights / gobos)
topic: viewer
status: ready
origin: render-feature gap analysis vs Firestorm (2026-07); split from viewer-projector-lights
---

Context: [context/viewer.md](../context/viewer.md).

Lights that **project a texture** — the slide-projector / gobo lights builders
use for spotlights, window light, caustics, logos on a wall. We render plain
point lights (P25), but a projector is a distinct thing: a spotlight with a
direction, a cone / FOV, and a texture sampled across the projected frustum.

Scope: read the projector parameters off the light (`LightImage` texture,
direction, FOV / cone, focus / falloff — the `Object` light-projection fields),
and render the light as a spotlight that modulates by the projected texture
sampled in the light's frustum. Cap the number of active projectors like point
lights, reusing the P25 nearest-N selection.

Per-projector **shadows** — the third shadow tier — are a separate follow-up in
[[viewer-projector-lights-spot-shadows]]; this task delivers the textured
spotlight without shadow casting.

Reference (Firestorm, read-only): the deferred spotlight / projector path,
`RenderSpotLightsInNondeferred` for the cheap path.

Builds on: P25 point lights (the light ingest and nearest-N selection).
