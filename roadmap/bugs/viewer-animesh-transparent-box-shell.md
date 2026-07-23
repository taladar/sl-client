---
id: viewer-animesh-transparent-box-shell
title: Animesh surrounded by an almost-transparent box shell
topic: viewer
status: bugs
origin: user report during the p29-2 aditi verification (2026-07-23)
refs: [viewer-p29-2, viewer-r25]
---

Context: [context/viewer.md](../context/viewer.md).

With the p29-2 fix in (the aditi Mario animeshes animate), each Mario is
**surrounded by an almost completely transparent box** that should
presumably not be visible at all.

To investigate, comparing against Firestorm on the same objects:

- Most likely the animesh **root prim's own box geometry**: an animesh
  linkset's root is commonly a plain prim made fully transparent, with the
  rigged mesh in a child. Check what Firestorm renders for that root —
  whether a fully-transparent face is *culled* there (its alpha pass drops
  `alpha = 0` faces) while we still draw the blend-pass box at alpha ≥
  1/255, or whether the root prim of an animesh is hidden outright
  (`LLControlAvatar` / `LLVOVolume::isAnimatedObject` render path).
- Note the [[viewer-r25]] interaction: before R25 a tinted-transparent
  prim carrying a `NONE`-mode legacy material rendered *opaque*; the R25
  fix put such faces back in the blend pass — if the shell was invisible
  before R25 and ghostly after, the box is a tinted-transparent prim and
  the question is purely why Firestorm's blend pass shows less of it
  (e.g. a full-transparency cull threshold, or fullbright/gamma
  difference on near-zero alpha).
- Pick the shell with `P` (the R25 dump prints the tint alpha, resolved
  alpha mode, and any legacy material) to pin which case it is.
