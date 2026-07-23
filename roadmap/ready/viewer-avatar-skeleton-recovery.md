---
id: viewer-avatar-skeleton-recovery
title: Undeform / reset skeleton for the own avatar
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
refs: [viewer-avatar-debug-tools, viewer-p18-3]
---

Context: [context/viewer.md](../context/viewer.md).

Recovery actions for a broken avatar rig: Avatar ▸ Avatar Health ▸
"Undeform Avatar" clears deformer/animation-driven skeleton offsets left
behind by malicious or buggy deformer animations; "Reset skeleton and
animations" rebuilds the base skeleton (including joint overrides) and
restarts the active animation set. Our avatar context menu already has a
reset-skeleton action for *other* avatars (`avatar_menu.rs`); the
self-targeted pair is missing.

Scope:

- Undeform: play the reference's undeform motion / clear accumulated
  joint offsets on the own avatar so the rig returns to its authored
  pose.
- Reset skeleton and animations: rebuild the own skeleton from the
  current shape + attachments (re-applying mesh joint offsets) and
  restart active animations.
- Menu entries under Avatar ▸ Avatar Health; reuse the existing
  reset-skeleton implementation where it already exists for others.

Reference (Firestorm, read-only): `Tools.Undeform`,
`Avatar.ResetSelfSkeletonAndAnimations` (`menu_viewer.xml` Avatar ▸
Avatar Health).

Builds on: the skeleton driver (done) and the avatar debug actions
cluster ([[viewer-avatar-debug-tools]]).
