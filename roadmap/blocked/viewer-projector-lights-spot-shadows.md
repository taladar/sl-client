---
id: viewer-projector-lights-spot-shadows
title: Projector spot-shadow tier
topic: viewer
status: blocked
origin: render-feature gap analysis vs Firestorm (2026-07); split from viewer-projector-lights
blocked_by: [viewer-projector-lights-textured]
---

Context: [context/viewer.md](../context/viewer.md).

The per-projector shadow tier on top of the textured spotlight
([[viewer-projector-lights-textured]]). Firestorm treats projector shadows as
the **third** shadow tier (`RenderShadowDetail` = "Sun/Moon + Projectors"), with
a separate deferred spot-shadow path.

Scope: add a per-projector shadow map so a projector casts its own shadows,
wired as the "+Projectors" `RenderShadowDetail` tier and capped like the
projector count itself.

Reference (Firestorm, read-only): the deferred spot-shadow path,
`RenderDeferredSpotShadowBias` / `RenderDeferredSpotShadowOffset` /
`RenderSpotShadowOffset`, `RenderShadowDetail`.

Builds on: [[viewer-projector-lights-textured]] (the spotlight and its frustum)
and P24 shadow maps.
