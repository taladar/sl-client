---
id: viewer-p31-11
title: Auto-stop flying on landing
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Done: `drive_avatar_controls` (`movement.rs`) now clears
`AvatarControls::flying` on landing via the pure, unit-tested
`should_auto_stop_flying` — flying, no ascend key (PageUp), descending
(PageDown held or vertical speed past `LANDING_DESCENT_SPEED_MPS`), and
`AvatarMotion::at_ground_floor` (position within `LANDING_HEIGHT_MARGIN_M`
of the stricter avatar ground floor `land + 0.5*height`). Requiring a
descent — not just the absence of lift — keeps pressing **F** to take off
from the ground from re-landing the avatar. Clearing the flag before the
flag set is assembled drops `ControlFlags::FLY` from the next
`SetControls`, so the P31.6 locomotion fallback also leaves the fly /
hover states.

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
