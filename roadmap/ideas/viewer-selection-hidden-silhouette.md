---
id: viewer-selection-hidden-silhouette
title: The selection outline does not show through intervening geometry
topic: viewer
status: ideas
origin: noticed porting the silhouette edge walk (2026-09-08)
refs: [viewer-outline-swallows-thin-hollow-prim, viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

With the Build floater open, the reference draws a selected object's outline
**twice**: once normally, and once with the depth test inverted so the part of
the outline hidden behind other geometry still shows, at 40% alpha and in
additive blend. `LLSelectNode::renderOneSilhouette`:

```cpp
if (LLSelectMgr::sRenderHiddenSelections)
{
    gGL.blendFunc(LLRender::BF_SOURCE_COLOR, LLRender::BF_ONE);
    LLGLDepthTest gls_depth(GL_TRUE, GL_FALSE, GL_GEQUAL);
    // … the same silhouette vertices as lines, colour at alpha 0.4
}
```

`sRenderHiddenSelections` follows the `RenderHiddenSelections` setting
(default **on**) and `wireframe_selection` is set whenever the tools floater
is visible, so in ordinary building this is what you see: an object you have
selected stays findable behind a wall. This viewer draws only the visible
pass, so a selected prim behind something is simply not outlined there.

The port now has the geometry it needs —
[[viewer-outline-swallows-thin-hollow-prim]] built the silhouette edge set,
and the hidden pass draws the *same* edges as plain lines. What it does not
have is a lever for the depth comparison:
`StandardMaterial` (and so `FaceMaterial`) exposes no `depth_compare`, so a
`GL_GEQUAL` pass needs either a second material type with its own pipeline
specialization or a render-phase item of its own. That, plus the additive
blend, is the whole of the work; the setting itself belongs in the debug
settings alongside the other `Render*` toggles.
