---
id: viewer-stop-all-animations
title: Stop all avatar animations (+ revoke variant)
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
refs: [viewer-p18-3, viewer-permission-active-grants]
---

Context: [context/viewer.md](../context/viewer.md).

The "I'm stuck in a pose" fix: Avatar ▸ Avatar Health ▸ Stop Avatar
Animations (Alt+Shift+A) force-stops every animation playing on the own
avatar; the second variant additionally revokes the animation
permissions scripts hold, so the animation cannot immediately restart.

Scope:

- Stop action: send `AgentAnimation` stops for every animation currently
  active on the own avatar (from the local animation registry) and reset
  to the default stand.
- Stop-and-revoke variant: additionally send permission revocation
  (`RevokePermissions`) to the objects holding `TRIGGER_ANIMATION` /
  `OVERRIDE_ANIMATIONS` grants — surfacing the grant registry that
  [[viewer-permission-active-grants]] tracks.
- Menu entries under an Avatar Health submenu + shortcut.

Reference (Firestorm, read-only): `Tools.StopAllAnimations` params
`stop`/`stoprevoke` (`menu_viewer.xml` Avatar ▸ Avatar Health).

Builds on: the animation driver (done) and the permission-grant
classifier (done); the active-grants management UI is the separate
[[viewer-permission-active-grants]].
