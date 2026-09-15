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

## Parity-audit addendum (2026-08-19)

CORRECTION: the task body claims the other-avatar reset action already
exists — it does not; the other-avatar entries are UNIMPLEMENTED
placeholders. The reference exposes **Reset Skeleton / Reset Skeleton &
Animations / Reset Mesh LOD** on OTHER avatars too
(menu_avatar_other.xml and both attachment menus — our shared
SELF_RESET_PIE placeholders in `avatar_menu.rs` / `attachment_menu.rs`),
plus Reset Skeleton on animesh objects (menu_object.xml; our object
RESET_PIE `reset-skeleton` slice in `object_menu.rs`). Extend the scope
beyond the own avatar to: other-avatar reset, animesh-object reset,
and **Reset Mesh LOD** (no coverage anywhere today).

## Addendum (2026-09-15): held joints make Reset Skeleton load-bearing

Since [[viewer-avatar-face-bone-shape-brow-spike]] a joint keeps the last value
any animation gave it, as in the reference, so a Bento pose left behind by a
stopped animation (a curled hand, an open jaw) stays until another animation
keys the joint. The reference's only other way out is `resetSkeleton`, which
rebuilds every joint's rest transform. A reset here must therefore also clear
the avatar's held pose: `AnimationPlayback::held` on the CPU and the slot's
held rows on the GPU (freeing and re-taking the slot, or a fresh occupancy
stamp, zeroes them).
