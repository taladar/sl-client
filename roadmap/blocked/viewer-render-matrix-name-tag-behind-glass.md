---
id: viewer-render-matrix-name-tag-behind-glass
title: Render matrix — a name tag seen through a translucent prim
topic: viewer
status: blocked
origin: viewer-render-context-matrix (2026-11-08) — curated pair not built
points: 2
refs: [viewer-render-context-matrix, viewer-name-tags-billboard-render, viewer-nametags-refracted-by-distant-water]
blocked_by: [viewer-render-hud-and-actor-axes]
---

Context: [context/testing.md](../context/testing.md).

The "name tag behind glass" curated pair: a name-tag subject with a
`Layering::InFrontOf(TranslucentPrim)` wall between it and the eye. Name
tags are billboards drawn by their own pipeline, which pulls them toward
the camera past the avatar's head. So whether they sort with the
translucent pass, or jump in front of glass they are behind, is a property
of that pipeline and not of the prims around it. Tags have broken this way
before ([[viewer-nametags-refracted-by-distant-water]]).

- Needs the `name-tag` subject and `ActorKind::NameTag` from
  [[viewer-render-hud-and-actor-axes]].
- Expectation: `SeeThrough` (the tag's colour and the wall's at its
  centre). Also add an opaque-wall twin that must read `Hidden`, unless the
  reference viewer draws tags over opaque geometry, in which case write
  that down as the rule with the Firestorm source it comes from.
