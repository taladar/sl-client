---
id: viewer-viewer-effect-render
title: Render inbound ViewerEffects (beams, spheres)
topic: viewer
status: ready
origin: script-interface survey (2026-07-23)
refs: [viewer-lookat-faithful, viewer-beacons-beam-render, api-g2]
---

Context: [context/viewer.md](../context/viewer.md).

`ViewerEffect` messages carry transient visual effects between viewers:
edit/selection beams from avatars editing objects, spheres, pointing
beams, and script-triggered effects. We decode them ([[api-g2]],
`Event::ViewerEffect`) and *send* our own LookAt/PointAt
(`look_at.rs`/`reach.rs`), but render none of the inbound ones — other
avatars' edit beams and effect spheres are invisible.

Scope:

- Render inbound effect kinds the reference draws: Beam (avatar hand →
  target, the classic editing beam), Sphere, PointAt/LookAt targets of
  other avatars (visualisation beyond the head-turn that
  [[viewer-lookat-faithful]] covers), Edit (selection sparkle).
- Effect colour, duration, and expiry per the message fields; cap the
  active-effect count defensively.
- Send-side completeness: emit our own Beam effect while editing an
  object, and the optional give-inventory particle beam the reference
  offers (FS `FSCreateGiveInventoryParticleEffect`).

Reference (Firestorm, read-only): `LLHUDManager::processViewerEffect`,
`llhudeffectbeam.cpp`, `llhudeffectlookat.cpp`, `llhudeffectpointat.cpp`.

Builds on: the decoded effect events and the beam-drawing machinery
shared with [[viewer-beacons-beam-render]].
