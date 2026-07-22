---
id: viewer-double-click-teleport
title: In-world double-click teleport
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22); split from viewer-click-to-walk-autopilot
blocked_by: [viewer-input-action-map]
refs: [viewer-autopilot-click-to-walk]
---

Context: [context/viewer.md](../context/viewer.md).

The Firestorm-style alternative to click-to-walk: double-click on ground /
a non-interactive surface teleports the avatar to the clicked point
(within-region `TeleportLocationRequest`), instead of walking there.

- The user setting chooses the double-click behaviour per the reference's
  `walk_to` / `teleport_to` pair — Firestorm's `FSDoubleClickAction`
  selects off / walk / teleport; the walk arm lands in
  [[viewer-autopilot-click-to-walk]], this task provides the teleport arm
  and the setting surface itself.
- Reuses the same click-classification path as click-to-walk (clicks on
  interactive objects — touch handlers, sit targets — keep their
  existing meaning; only bare ground / non-interactive surfaces trigger).
- Height handling matches the reference: teleport to the picked surface
  point, look-at preserved (`teleportViaLocationLookAt`).

Related double-click teleport surfaces, kept consistent but implemented
in their own tasks: the world map (`viewer-world-map-tracking-teleport`)
and the minimap (its interactions task) — all three should drive the
same teleport/tracking backend rather than three ad-hoc paths.

Reference (Firestorm, read-only): `lltoolpie` (double-click dispatch),
`llagent` (`teleportViaLocationLookAt`), setting `FSDoubleClickAction`.

Builds on: the picking path (`avatar_pick.rs` / object picking) and the
existing teleport plumbing (`protocol-10`).

Deps: [[viewer-input-action-map]] (the gesture binding).
