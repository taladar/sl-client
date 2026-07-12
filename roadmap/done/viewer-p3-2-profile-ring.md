---
id: viewer-p3-2
title: Profile ring
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 3 — `sl-prim` (pure Linden prim tessellation)
---

Context: [context/viewer.md](../context/viewer.md).

**P3.2. Profile ring.** `profile.rs`: 2D profile (square / circle /
half-circle / triangles) via `genNGon`, with profile begin/end cut and hollow
(`addHole`) plus the semantic face-index ranges. A `Profile` of
`ProfilePoint`s (2D position + sweep-parameter `u`) and `ProfileFace` ranges
(`index`/`count`/`scale_u`/`cap`/`flat` + a `ProfileFaceId` `LL_FACE_*` bit
flag), built by a private `Builder` mirroring `LLProfile::generate` /
`genNGon` / `addHole` / `addCap` (per-edge `split`, path caps, open-ring
profile edges, sphere-close special case).
