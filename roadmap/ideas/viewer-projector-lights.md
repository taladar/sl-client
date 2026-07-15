---
id: viewer-projector-lights
title: Projector lights (textured spotlights / gobos)
topic: viewer
status: ideas
origin: render-feature gap analysis vs Firestorm (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Lights that **project a texture** — the slide-projector / gobo lights builders
use for spotlights, window light, caustics, logos on a wall. We render plain
point lights (P25), but a projector is a distinct thing: a spotlight with a
direction, a cone/FOV, and a texture sampled across the projected frustum, and
it can cast its own shadows.

Firestorm treats projector shadows as the third shadow tier
(`RenderShadowDetail` = "Sun/Moon + Projectors"), with a separate deferred
spot-shadow path (`RenderDeferredSpotShadowBias` / `Offset`,
`RenderSpotShadowOffset`) and `RenderSpotLightsInNondeferred` for the cheap
path.

Scope: read the projector parameters off the light (`LightImage` texture,
direction, FOV/cone, focus/falloff — the `Object` light-projection fields), and
render the light as a spotlight that modulates by the projected texture sampled
in the light's frustum; add the per-projector shadow map for the "+Projectors"
tier. Cap the number of active projectors like point lights.

Reference (Firestorm, read-only): the deferred spotlight / projector path, the
`Render*SpotShadow*` settings.

Builds on: P25 point lights (the light ingest and nearest-N selection) and P24
shadow maps.
