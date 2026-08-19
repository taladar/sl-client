---
id: viewer-build-stretch-textures
title: Stretch tool — Stretch Textures toggle
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-transform-gizmos, viewer-stretch-global-axis-object,
       viewer-prim-texture-editing]
---

Context: [context/viewer.md](../context/viewer.md).

The build tool row's "Stretch Textures" checkbox
(`ScaleStretchTextures`): while scaling a prim, rescale each face's
texture repeats along with the face, so the texture stretches with the
surface instead of tiling more. With the toggle off, repeats stay
fixed — which is our current and only behaviour (no stretch-textures
hit anywhere in `sl-client-bevy-viewer/src/gizmos.rs`).

Applies in the stretch gizmo's commit path (and equally when the Size
numeric fields change scale), sending the updated TextureEntry
alongside the scale via the existing texture-editing spine
([[viewer-prim-texture-editing]]). The reference implementation lives
in llmanipscale's stretch-face texture rescale.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml` (L377),
`indra/newview/llmanipscale.cpp`.
