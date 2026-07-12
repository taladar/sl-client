---
id: viewer-r20
title: Directional shadows oscillate along one axis
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R20. Directional shadows oscillate along one axis**
(`sl-client-bevy-viewer`, P24.1). **Fixed.** Noticed while verifying P25.2
local lights: with a static camera and a stationary light prim, the sun/moon
cascaded shadows on the ground jittered back and forth a small amount along a
single axis, frame to frame. **Root cause** (confirmed by logging the
per-frame light direction — 3196 unique values across 3221 frames): the day
cycle runs off the real-time clock (`day_position` reads `SystemTime::now()`),
so the sun rotates a hair **every frame**. Bevy's cascaded shadow maps already
texel-snap the cascade origin
(`bevy_light::build_directional_light_cascades` floors `near_plane_center` to
texel multiples), but that snap is done in **light space** — a per-frame
rotating light rotates the snap grid itself, so a fixed receiver lands on a
different texel each frame and the shadow shimmers / oscillates (the
back-and-forth is the `floor()` flip-flopping at a texel boundary). **Fix:**
`snap_shadow_direction` (sky.rs) quantises the **shadow-caster** direction to
a texel-equivalent angular grid (round the unit-vector components to
`1 / shadow_map_size` and re-normalise) before orienting the `SceneSun`
`DirectionalLight`. The direction is then bit-identical across the frames
whose true direction stays in one cell (verified: it now holds for ~10–36
frames even at fast dawn, far longer midday), so Bevy's texel snapping keeps
the shadow perfectly still; each step moves any cascade's shadow by ≤ ~1 texel
(imperceptible). Only the shadow projection is snapped — the visible sun disc,
sky, and light colour keep the continuous direction. Verified live on OpenSim.
Independent of Phase 25.
