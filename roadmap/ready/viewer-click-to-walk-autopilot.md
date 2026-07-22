---
id: viewer-click-to-walk-autopilot
title: Click-to-walk, double-click teleport, autopilot
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

Point-and-go movement:

- **Autopilot core**: steer the avatar toward a target point via the same
  agent-control bits the keyboard path uses, with arrival detection,
  obstacle give-up, and cancel-on-user-input. (The reference's
  `LLAgent::startAutoPilotGlobal`.)
- **Click-to-walk**: single-click (or double-click, per setting) on ground /
  object walks there via the autopilot.
- **Double-click teleport**: the FS alternative — double-click teleports to
  the clicked point (within-region `TeleportLocationRequest`), with the
  setting choosing between off / walk / teleport per the reference's
  `walk_to` / `teleport_to` pair.
- Scripted autopilot requests (`ScriptTeleportRequest`-style go-to) can ride
  the same core later.

Mind the interaction rules: clicks on interactive objects (touch handlers,
sit targets) keep their existing meaning; the walk trigger only fires on
bare ground / non-interactive surfaces, matching the reference.

Reference (Firestorm, read-only): `llagent` autopilot, `lltoolpie`
(click-to-walk / double-click handling), settings `FSDoubleClickAction`.

Builds on: the picking path (`avatar_pick.rs` / object picking) and the
movement control bits (`movement.rs`).
