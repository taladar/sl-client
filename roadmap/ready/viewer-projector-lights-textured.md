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

**Audit 2026-10-15: the untextured half has landed.** The light-image
extra-param is decoded into `LightProjection` (texture, fov, focus, ambiance;
`sl-viewer-world-api/src/object_components.rs`), and
`sl-viewer-world-scene/src/lights.rs` (`drive_local_lights` / `spot_light`)
already renders a projector as a Bevy `SpotLight` with its cone, inside the
shared nearest-N budget. What remains is the projected **texture** (Bevy 0.19
has `SpotLightTexture` in `bevy_light`; check whether it fits before building
a custom path),
plus `focus` and `ambiance`.

Per-projector **shadows** — the third shadow tier — are a separate follow-up in
[[viewer-projector-lights-spot-shadows]]; this task delivers the textured
spotlight without shadow casting.

Reference (Firestorm, read-only): the deferred spotlight / projector path,
`RenderSpotLightsInNondeferred` for the cheap path.

Builds on: P25 point lights (the light ingest and nearest-N selection).
