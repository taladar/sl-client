---
id: viewer-p31-11
title: Auto-stop flying on landing
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.11. Auto-stop flying on landing.** In the reference viewer, flying
down to the ground automatically turns flight off — once the avatar touches
(or nears) the ground / a surface, the viewer clears the fly state so the
agent lands and stands rather than hovering just above the ground still in
fly mode. Our viewer's P31.5 fly toggle (`movement.rs`, **F**) never clears
itself: it stays `flying = true` until the user presses **F** again, so a
descent to the ground leaves the avatar stuck in a hovering fly state. Detect
the landing — the reference path is `LLAgent::setFlying(FALSE)` driven from
the fly state machine (`FS_STATE`/`fly` handling in `llagent.cpp`) when the
agent is on / very close to the ground while descending (roughly: fly is on,
`UP_NEG`/no lift, and the dead-reckoned height is at the ground floor), and
the sim itself also stops broadcasting the fly animation — and clear
`AvatarControls::flying` (dropping `ControlFlags::FLY` from the advertised
intent, which also lets the P31.6 locomotion fallback fall back out of the
fly / hover states). Keep the manual **F** toggle for taking off again.
Viewer-only (movement / control-flag plumbing already exists end-to-end).
Reference: `LLAgent::setFlying` / the ground-detect in `llagent.cpp`.
