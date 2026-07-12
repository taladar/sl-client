---
id: viewer-p16-1
title: Detect & parent
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 16 — Attachments (rigid)
---

Context: [context/viewer.md](../context/viewer.md).

**P16.1. Detect & parent.** In `objects.rs` `reconcile_parent`, when an
object's `parent_id` resolves to a **pcode-47 avatar** (not a prim linkset),
decode `attachment_point()`, look up that avatar's skeleton **joint entity**
(Phase 13), and parent the attachment there via `ChildOf` so it follows the
posed skeleton. Hold-pending when the avatar/joint is not present yet (reuse
the existing pending-adoption path). **Done:** `apply_object` marks an object
whose `attachment_point_id()` is set as an attachment (its `parent` is the
avatar) and holds it parentless rather than reconciling a linkset root; a
companion `adopt_pending_attachments` system (the pending-adoption pattern,
in its own system because the avatar's skeleton lives in `AvatarState` /
`AvatarBody`, resources `update_objects` cannot reach and which are populated
by a later system) resolves each pending attachment's target joint —
raw point id → skeleton joint index (`AvatarBody::attachment_joint_index`,
from the `avatar_lad.xml` `<attachment_point>` table now parsed into
`AvatarAssetLibrary`) → the avatar's joint entity
(`AvatarState::attachment_joint_entity`, from a new per-agent joint-entity
store) — and `ChildOf`-parents it, retrying on later frames until the
avatar/joint exists. A sphere-only (no `--viewer-assets`) avatar has no
skeleton, so its attachments fall back to the avatar object entity (position
only), preserving the pre-P16 behaviour. **Synthetic `mRoot`:** the reference
viewer creates an `mRoot` joint above `mPelvis` in code (it is not in
`avatar_skeleton.xml`), so the avatar-centre attachment point
(`joint="mRoot"`) had no joint to resolve to;
`BevySkeleton::insert_synthetic_root`
appends an identity root above the former roots (indices unchanged), which the
viewer adds after building the skeleton — with it all 47 non-HUD attachment
points resolve to a real joint (8 HUD points, whose `mScreen` is not a body
joint, stay unresolved for Phase 19). Verified live on OpenSim: assets load
(134 joints incl. `mRoot`, 55 attachment points) and the rigged avatar shapes
cleanly across 134 joints with no panic from the new systems; the
attachment-*tracks-the-avatar* live check (needs a worn attachment) is
P16.2's.
