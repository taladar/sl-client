---
id: viewer-p16-2
title: Attachment transform
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 16 — Attachments (rigid)
---

Context: [context/viewer.md](../context/viewer.md).

**P16.2. Attachment transform.** Place the attachment at its stored local
offset/rotation relative to the joint; honour attachment `ADD_FLAG` vs
replace. **Done:** the reference viewer models each attachment point as a
node parented to its skeleton joint at the fixed `avatar_lad.xml`
`position` / `rotation` offset (`LLViewerJointAttachment`), with the worn
object's own local transform relative to *that node* — not the bare joint. So
P16.1's direct joint-parenting seated an attachment at the joint origin,
missing the point offset (e.g. the Chest point sits `0.15 0 -0.1`, rotated
`0 90 90`, off `mChest`). `AttachmentPointInfo` now carries each point's
offset (`avatar_assets.rs`), `AvatarBody` resolves it into a
`BodyAttachmentPoint { joint_index, offset: Transform }`, and `spawn_body`
spawns one **attachment-point node** entity per point as a child of its joint
carrying that offset (a new per-agent `AvatarState::attachment_nodes` store,
despawned with the body). `adopt_pending_attachments` now parents a worn
attachment to the node (`attachment_point_entity`) instead of the joint, so
the object's existing child transform (local pos/rot in Second Life Z-up)
composes onto the point offset — the full joint → point → object chain. The
offset is kept in the joint's Second Life Z-up frame (no basis change), like a
linkset child's local transform; a new `coords::sl_euler_deg_to_quat`
reproduces `LLQuaternion::setQuat(roll, pitch, yaw)` verbatim so the point
rotation matches the reference viewer exactly (unit-tested vs the glam
single-axis quaternions). **`ADD_FLAG`:** nothing to honour on the render
side — the transient `ATTACHMENT_ADD` (`0x80`) bit is already stripped in
`sl-proto`'s `attachment_point_from_state`, and add-vs-replace is a
server-side inventory concern (a replaced attachment is removed by
`KillObject`, handled via `ObjectRemoved`); the viewer simply renders every
attachment the server streams on its point. **Verified live on OpenSim:** a
cube worn at the Chest point (local pos `0,0,0`, so it seats exactly at the
chest node's offset from `mChest`) on one avatar is seen by a second observer
avatar's viewer, which spawns both rigged bodies and logs `parented
attachment … (point 1) to avatar … joint` with no panic from the new
node-spawning path.
