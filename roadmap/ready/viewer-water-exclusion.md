---
id: viewer-water-exclusion
title: Water-exclusion surfaces (invisiprim successor)
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-r25]
---

Context: [context/viewer.md](../context/viewer.md).

Water-exclusion surfaces: faces carrying the sentinel "invisible" texture
(the legacy invisiprim UUIDs) punch a hole in the **water plane** — the
modern reference repurposed the old invisiprim pass into a dedicated
water-exclusion draw pool, and boat / dock content relies on it to keep
hulls dry. Without it, such prims render as odd solids (they currently fall
through as ordinary textured faces).

Scope: detect the sentinel texture ids on faces at ingest, exclude those
faces from normal rendering, and mask the water surface where they are
(reference approach: render exclusion volumes into a mask the water shader
samples). Include the **legacy invisiprim** behaviour question explicitly:
old content also expected avatar/sky occlusion — decide and document how
far we follow the modern reference (which dropped that part) vs legacy
(per the support-legacy-content policy, match today's reference: water
exclusion only).

Reference (Firestorm, read-only): `lldrawpoolwaterexclusion.{cpp,h}`,
`PASS_INVISIBLE`, `llviewertexturelist` (sentinel ids), `llvowater`.

Builds on: the water renderer (`water.rs`) and face ingest (`textures.rs`).
