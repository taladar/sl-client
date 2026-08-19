---
id: viewer-movement-quickjump-movelock
title: Movelock and Quickjump movement toggles
topic: viewer
status: blocked
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-fs-bridge-protocol]
refs: [viewer-p31-5, viewer-qol-toggles]
---

Context: [context/viewer.md](../context/viewer.md).

Two Firestorm movement toggles under Avatar ▸ Movement:

- **Movelock** (Ctrl+Alt+P, `Self.ToggleMoveLock`): pin the avatar in
  place against pushes and drift. Implemented via the FS LSL bridge
  (`llMoveToTarget` anchoring from the bridge attachment), with
  relock-after-move behaviour settings.
- **Quickjump** (`Self.toggleIgnorePreJump`): skip the pre-jump crouch
  animation so jumps fire instantly.

Scope: both toggles in the Avatar ▸ Movement menu + settings; Quickjump
is a local movement-controller change (suppress the pre-jump phase);
Movelock issues the bridge command and re-anchors on position change per
the reference behaviour.

Reference (Firestorm, read-only): `menu_viewer.xml` Avatar ▸ Movement,
`fslslbridge.cpp` (movelock commands), the `FSMovelock*`/pre-jump
settings.

Builds on: the FS bridge protocol (blocked task) for movelock; the
movement controller ([[viewer-p31-5]], done) for quickjump.

## Parity-audit addendum (2026-08-19)

Additions from the audit: `RelockMoveLockAfterRegionChange` semantics —
on a region change the movelock either re-locks at the destination or
auto-disables with a notice (FIRE-11924) depending on the setting; and
the small on-screen "Movelock is enabled" indicator Firestorm draws
(llviewerwindow.cpp:879-883). Our movelock notifications are already
ported (`sl-client-bevy-viewer/src/notifications.rs:3702-3746`).

Parity-audit extension: crouch-as-toggle mode (`FSCrouchToggle`, with
`FSCrouchToggleStatus` holding the current state) — pressing the crouch
key once toggles crouching instead of requiring hold.
