---
id: viewer-prim-attachment-worn-but-not-rendered
title: A plain prim attachment reads "(worn)" in inventory but never appears on
  the avatar
topic: viewer
status: done
origin: drag-to-world verification of the feature-tier flatten, local OpenSim
  (2026-08-26)
refs: [viewer-hud-attachments-not-composited,
  viewer-rigged-attachments-wearer-not-resolved, viewer-object-wear-attach,
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

## RESOLVED (2026-09-09): the body-attachment path is sound; the report was a HUD

Investigated end to end on the local grid and offline. **A plain prim worn on a
body point renders.** What the report actually hit is
[[viewer-hud-attachments-not-composited]]: nothing worn on a **HUD** point
reaches the frame, and the item that was attached went to a HUD point.

### Why a "plain prim cube" landed on a HUD point

The attach sends `AttachmentPoint::Default`, and the simulator resolves that to
the item's **remembered** point — `AttachObjectInternal`
(`OpenSim/Region/CoreModules/Avatar/Attachments/AttachmentsModule.cs`) takes
`group.AttachmentPoint`, then `Shape.LastAttachPoint`, and only falls back to
Left Hand when both are zero. The test avatar's object items had last been worn
on HUD points by earlier sessions, so `Default` put them back there. Live, with
`SL_VIEWER_LOG_ATTACHMENT_BIND=1`, the two cubes that "did not appear" arrived
on points **31** and **35** (HUD Center 2, HUD Center) while the sculpt that did
appear arrived on point **5** (Left Hand). The split is HUD-versus-body, not
prim-versus-sculpt, and the reference resolves `Default` the same way — there is
nothing to fix on this side.

### What was checked, and how

Each of the task's four "next diagnostics" is now answered:

1. **Does the object arrive?** Yes, correctly. Worn via `sl-repl` on Right Hand,
   Chest, Left Hand (from `Default`) and HUD Center, the object stream carries
   `parent_id` = the avatar's own object and the nibble-swapped `state` bytes
   `96`/`16`/`80`/`50`, which un-swizzle to points 6/1/5/35. OpenSim packs the
   point into `state` for **every** part of an attachment linkset
   (`LLClientView.CreatePrimUpdateBlock`), and our `attachment_point_from_state`
   matches the reference's `ATTACHMENT_ID_FROM_STATE`.
2. **Is it parented to the right entity?** Yes — `adopt_pending_attachments`
   seats it on the wearer's attachment-point node, now traced
   (`attachment … seated on point 1 of avatar …`).
3. **Is it built and visible?** Yes — both body cubes draw on the avatar in a
   headless capture.
4. **Is `Default` the problem?** No, see above — but it is what steered the
   report to a HUD point.

### What this left behind

- `SL_VIEWER_LOG_ATTACHMENT_BIND=1` now traces the **whole** journey, not just
  the rigged bind: the object layer logs a worn object's arrival, and the
  seating pass logs where each attachment was seated / routed, or which of the
  three stalls (wearer untracked, no body, no node for the point) is holding it.
- `sl-viewer-kit`'s `vendored_avatar_attachment_points` pins that every one of
  `avatar_lad.xml`'s 55 points resolves — the body points to a real skeleton
  joint, ids 31–38 to the HUD screen — so a point that silently has no node
  fails as a test rather than as one invisible attachment.
- `rigged_attachments`' own tests pin that a worn prim is seated on its wearer's
  node for its point, and that a point with no node stays pending rather than
  being dropped.
- `world_test`'s `a_prim_rezzed_in_world_and_then_worn_moves_onto_its_wearer`
  pins the mid-session path the report used: the object is streamed first as an
  in-world root and then as a worn attachment, and must move onto its wearer and
  lose the world-root re-base marker.
