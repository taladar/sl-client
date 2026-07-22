---
id: viewer-autopilot-click-to-walk
title: Autopilot core + click-to-walk
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22); split from viewer-click-to-walk-autopilot
blocked_by: [viewer-input-action-map]
refs: [viewer-double-click-teleport]
---

Context: [context/viewer.md](../context/viewer.md).

Point-and-go movement (the walking half; the teleport alternative is
[[viewer-double-click-teleport]]):

- **Autopilot core**: steer the avatar toward a target point via the same
  agent-control bits the keyboard path uses, with arrival detection,
  obstacle give-up, and cancel-on-user-input. (The reference's
  `LLAgent::startAutoPilotGlobal`.)
- **Click-to-walk**: single-click (or double-click, per setting) on ground /
  object walks there via the autopilot.
- Scripted autopilot requests (`ScriptTeleportRequest`-style go-to) can ride
  the same core later.

Mind the interaction rules: clicks on interactive objects (touch handlers,
sit targets) keep their existing meaning; the walk trigger only fires on
bare ground / non-interactive surfaces, matching the reference. That
click-classification path is shared with [[viewer-double-click-teleport]]
— build it once.

Reference (Firestorm, read-only): `llagent` autopilot, `lltoolpie`
(click-to-walk handling), setting `FSDoubleClickAction` (selects between
walk / teleport / off for the double-click gesture — the setting spans
both tasks).

Builds on: the picking path (`avatar_pick.rs` / object picking) and the
movement control bits (`movement.rs`).
