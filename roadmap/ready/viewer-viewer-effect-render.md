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

## Parity-audit addendum (2026-08-19)

Parity-audit extension: emit the script-chat particle effect
(`EffectScriptChatParticles`) — the small viewer-side particle burst
above an object when it chats on a script channel (FS also broadcasts
selection info on channel 9000 under `FSParticleChat`, an old
inter-viewer convention; the render side belongs here).

## Measured (2026-10-13, [[gridspec-viewer-effects-sounds]])

What each grid passes on is in `book/src/gridspec/avatars.md` § Effects and
sounds. For this task: Second Life sends a viewer **its own effects back**
(skip the ones whose source is the own agent, as the gaze and the pointing
already do) and stamps every effect with the sender's agent id; it drops a
beam, a gaze or a pointing whose payload does not begin with the sender's id,
so the beam this task sends has to name the own avatar as its source. OpenSim
passes everything on as sent, but a message of nothing but gazes and beams
only to a viewer whose camera is within 10 m of the sender — send a beam in
a message of its own and a far viewer will not draw it there. The fake grid
relays effects by each flavour's rules, so a two-viewer e2e test can check
the rendering.
