---
id: viewer-p31-12a
title: Per-frame visual-param morph pipeline
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — split out of P31.12 (head & eye look-at)
refs: [viewer-p31-12, viewer-p31-12b, viewer-p34-1]
---

Context: [context/viewer.md](../context/viewer.md).

**P31.12a. Per-frame visual-param morph pipeline.** The viewer's appearance
pipeline resolves the shape / morph visual-params and **bakes the morph targets
into the base-mesh geometry once**, at appearance-change time
(`apply_avatar_appearance` → `MorphWeights::from_resolved` →
`to_bevy_morphed_mesh`), then hands the deformed mesh to the GPU as a static
asset. Several motions instead need a **subset of visual-params driven every
frame** on top of that baked appearance, which the current pipeline cannot do:

- the eye **blink** ([[viewer-p31-12b]]) drives `Blink_Left` / `Blink_Right`;
- the **physics wearable** body bounce ([[viewer-p34-1]]) drives the
  breast / belly / butt jiggle params from `avatar_lad.xml` per frame.

Add a per-frame morph capability so a small set of named visual-params can be
animated without re-baking the whole appearance each frame. The natural fit is
Bevy's native **morph-target** support (`MorphWeights` component + morph-target
attributes on the mesh) for just the runtime-driven params, layered over the
statically-baked base mesh — a per-frame CPU re-mesh of the whole body would be
far too costly. Decide the split between bake-time (static) and runtime
(animated) params, expose a resource/component to set a named param's weight per
frame, and keep the existing static bake for everything else. Reference:
`LLCharacter::setVisualParamWeight` / `updateVisualParams`, and the
`LLPolyMorphTarget` / driven-param plumbing in `llvoavatar.cpp`.
