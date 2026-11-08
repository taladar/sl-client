---
id: viewer-render-hud-and-actor-axes
title: HUD screen axis and the name-tag, particle and avatar actors
topic: viewer
status: ready
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-render-scene-coverage]
blocked_by: [viewer-render-context-matrix]
---

Context: [context/testing.md](../context/testing.md).

The first matrix ships prim actors only. Add the actors whose rendering
paths are their own subsystems, and the HUD screen:

- `Screen::Hud` — the subject is re-parented under a HUD root on the HUD
  render layer with an orthographic HUD camera targeting the same image
  at a higher order, so it composites over the world frame exactly as the
  viewer does; the "prim on HUD over prim in world" pair lands here.
- `ActorKind::NameTag` (`name_tag_render_bundle` + a plain-name content),
  `ActorKind::Particles` (the fountain system with a red texture),
  `ActorKind::Avatar` (the fixture base part), `ActorKind::Terrain` (a
  red composition) — each with the expectation rule it implies.
- New subjects: `name-tag` and `pbr-sphere` (`translucent-box` landed
  with the matrix as `prim-translucent-box`; `glow-prim` belongs to
  [[viewer-render-rig-glow-pass]]).

Acceptance: the `screen` slot and each new actor kind run in the R1
sweeps, each with a teeth case that fails as declared (for example, an
opaque name-tag actor in front of a subject must read `Hidden` if tags
occlude, and the HUD subject must vanish with its camera ordered below the
world's). The curated pairs built on them are their own tasks:
[[viewer-render-matrix-hud-over-world]],
[[viewer-render-matrix-name-tag-behind-glass]] and
[[viewer-render-matrix-translucent-grazing-terrain]].
