---
id: viewer-particle-editor
title: Particle-system editor
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-texture-picker, viewer-ui-color-picker]
refs: [viewer-prim-parameter-editing]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's particle editor: a form over every `ParticleSystem` field
(pattern, burst rate/count, speed, angle, radius, accel, colours + alpha
over lifetime, scale over lifetime, texture, wind/bounce/follow flags,
target) with a **live preview on a selected owned prim** — it writes the
particle ExtraParams to the object (encoder done: `protocol-56`) — and an
**LSL export** button generating the `llParticleSystem` call for the shown
settings (the feature creators actually use it for).

Colour fields via [[viewer-ui-color-picker]], texture via
[[viewer-ui-texture-picker]].

Reference (Firestorm, read-only): `fsfloaterparticleeditor` /
`floater_particle_editor.xml`.

Builds on: `protocol-56` ParticleSystem encoder + the P30 particle
renderer (instant local feedback).

Deps: [[viewer-ui-texture-picker]], [[viewer-ui-color-picker]].
