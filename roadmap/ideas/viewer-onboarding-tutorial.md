---
id: viewer-onboarding-tutorial
title: In-viewer tutorial for new and returning Second Life users
topic: viewer
status: ideas
origin: idea raised during viewer-avatar-state-dump-replay work (2026-08-06)
---

Context: [context/viewer.md](../context/viewer.md).

Add an optional, dismissible in-viewer **tutorial / onboarding** flow aimed at
two audiences:

- **New SL users** — never used a viewer: movement (WASD / arrows, run, fly,
  sit/stand), the camera (orbit/pan/zoom, mouselook), teleporting and the map,
  local vs group/IM chat, editing appearance (wearables, mesh bodies/heads,
  BoM), inventory basics, attaching/detaching, and the safety/etiquette basics
  (mute, report, derender).
- **Returning users after a long absence** — know the old viewer but not what
  changed: Bakes-on-Mesh, Bento/mesh heads & bodies, Environment (EEP) vs old
  WindLight, experiences, the current attachment-point/animation landscape, and
  whatever this viewer does differently from the reference.

Shape (to design): a first-run welcome that offers the tutorial, a non-blocking
step-by-step overlay that highlights the relevant UI and waits for the user to
perform each action (movement, camera, chat, appearance…), and a "Help →
Tutorial" entry to replay it anytime. Track completion per-account (the
per-avatar settings dir) so it only auto-offers once. Content should be
data-driven (a step list / resource) so it is easy to extend, localise, and
skin, and should degrade gracefully on OpenSim (features SL has that OpenSim
lacks are noted, not broken links).

Nice-to-haves: detect a likely-new avatar (default/starter shape, near-empty
inventory) or a long-dormant account to tailor which track is offered; a compact
"what's new" changelog surface for returning users.
