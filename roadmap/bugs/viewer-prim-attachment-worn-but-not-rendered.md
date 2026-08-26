---
id: viewer-prim-attachment-worn-but-not-rendered
title: A plain prim attachment reads "(worn)" in inventory but never appears on
  the avatar
topic: viewer
status: bugs
origin: drag-to-world verification of the feature-tier flatten, local OpenSim
  (2026-08-26)
refs: [viewer-rigged-attachments-wearer-not-resolved, viewer-object-wear-attach,
  viewer-inventory-attach-to-point]
---

Context: [context/viewer.md](../context/viewer.md).

Symptom, on the local OpenSim grid: attaching a **plain (non-rigged) prim
cube** from inventory marks the item **`(worn)`** in the inventory list, but no
cube ever appears on the avatar. Reproduced by two different entry points into
the same command — dragging the item onto the own avatar's body in-world
(self-drop → wear), and the inventory context menu's **Attach** — so it is not
the drag path.

Both entry points end at the same place: `wear_commands`
(`sl-viewer-inventory/src/inventory_actions.rs`) sends
`Command::RezAttachment` with `AttachmentPoint::Default` and
`AttachmentMode::Replace` for an `Object` / `Attachment` item. That the item
turns `(worn)` says the *wearables / COF* side of the round-trip completed; the
missing half is the **rendering** of the resulting attached object.

## What is and is not already known

- This is **not** [[viewer-rigged-attachments-wearer-not-resolved]]. That bug is
  about *rigged* attachments failing the wearer walk in
  `apply_rigged_attachments`; a plain prim is a normal `SceneObject` parented to
  the avatar's `AttachmentPointNode`, and never enters the rigged bind at all.
  They may still share a cause — an attachment whose parent linkage never
  resolves would break both — which is why that one is worth re-reading first.
- HUD attachments **do** render (the OpenSim test avatar's 0.5 m HUD cube is a
  standing fixture), so the attachment-point tables from `avatar_lad.xml`
  (`BodyAttachmentPoint`, `avatars.rs`) are populated and at least the HUD-layer
  path works. That narrows it to the **non-HUD** in-world attachment path.
- No warning or error is logged at `RUST_LOG=info` for the attach, so nothing
  currently reports the failure.

## Next diagnostics

1. Does the attached object **arrive at all**? Log the `ObjectUpdate` for the
   rezzed attachment: does the viewer track an object whose `parent_id` is the
   own avatar, with a non-`None` `attachment_point`? If it never arrives, the
   bug is on the wire / request side, not the render side.
2. If it arrives, is it **parented** to the right entity? An attachment should
   end up a child of the avatar's `AttachmentPointNode` for its point id
   (`avatars.rs:1499`); check whether `reconcile_parent` places it there or
   leaves it at the world root (where it would be drawn at the region origin
   rather than "missing" — worth looking for a stray cube at `<0,0,0>`).
3. If parented, is it **built and visible**? Check its `PendingGeometry` stage
   and `Visibility` — a prim needs no mesh fetch, so a prim stuck unbuilt would
   point at the attachment path suppressing the normal object build.
4. Check `AttachmentPoint::Default` specifically: the server picks the item's
   last-used point. If our tables key strictly on an explicit point id, a
   `Default` attach may resolve to a point node we never spawned.

Verify on both grids — OpenSim is where it was seen, and Second Life is the
primary target, so an aditi check is what decides whether it is grid-specific.
