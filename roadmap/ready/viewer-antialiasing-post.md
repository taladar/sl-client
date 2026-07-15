---
id: viewer-antialiasing-post
title: Post-process antialiasing (FXAA / SMAA / TAA selection)
topic: viewer
status: ready
origin: render-feature gap analysis vs Firestorm (2026-07); split from viewer-antialiasing
---

Context: [context/viewer.md](../context/viewer.md).

MSAA does not work with a deferred renderer, so SL antialiases with a
**post-process** pass. `RenderFSAAType` selects the algorithm (FXAA or SMAA) and
`RenderFSAASamples` the quality tier. Without it, SL's countless high-contrast
edges — prims against sky, alpha masks — shimmer and crawl as the camera moves.
This is the substantial piece of the image-sharpness family.

Scope: an FXAA and an SMAA post pass with a type/quality selector, behind the
graphics settings. Order matters — the AA resolve runs at the **end** of the
post chain, after tone mapping and any bloom / DoF, so it operates on the final
LDR image. Add the temporal (TAA) option as the higher-quality tier if the
deferred pipeline can supply the motion/history it needs.

Reference (Firestorm, read-only): the post-AA passes, `RenderFSAAType` /
`RenderFSAASamples`.

Builds on: the deferred pipeline's final resolve.
