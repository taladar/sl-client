---
id: viewer-double-click-teleport
title: In-world double-click teleport
topic: viewer
status: done
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

## Done (2026-08-07)

New `double_click_teleport.rs`: a persisted `DoubleClickAction` setting
(`0` nothing / `1` teleport / `2` walk), the reference **Ctrl+Shift+D** hotkey
(the `menu_viewer.xml` "DoubleClick Teleport" `Advanced.SetDoubleClickAction
teleport_to` shortcut) toggling it on/off, and a double-click detector with the
same UI / HUD occlusion + `Alt` guards as the world touch pick. The picked
point is resolved to the containing region (current **or** a visible neighbour)
via the shared `region_handle_at` / `narrow` map math and issued through the
shared `issue_teleport` backend, so it drives the same teleport + progress path
as the minimap and world map. Verified live on OpenSim (in-region and
cross-region).

Scope landed vs. deferred: the **teleport** arm + the setting + the hotkey (the
gesture detection must not be gated on keyboard focus — a mouse gesture is
independent of it, fixed live). The trigger fires on **terrain only** (an
object's script touch-handlers are not visible client-side, so bare ground is
the safe subset) — object-surface teleport is a follow-up. The **walk** arm is a
stub routing to [[viewer-autopilot-click-to-walk]]. Objects not rebasing on a
cross-region arrival is the separate
[[viewer-seamless-region-handover-objects]].
