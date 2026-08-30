---
id: viewer-render-hud-and-actor-axes
title: HUD screen axis and the name-tag, particle and avatar actors
topic: viewer
status: blocked
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
- New subjects: `name-tag`, `glow-prim`, `pbr-sphere`, `translucent-box`.

Acceptance: the curated pairs that need these (name tag behind glass,
particles at midnight, HUD over world, translucent grazing over terrain)
run and their teeth cases fail as declared.
