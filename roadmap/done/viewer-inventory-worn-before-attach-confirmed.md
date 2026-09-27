---
id: viewer-inventory-worn-before-attach-confirmed
title: Inventory's worn state for attachments follows requests, not the avatar
topic: viewer
status: done
origin: found while live-testing viewer-menu-touch-object (2026-09-27)
refs: [viewer-menu-touch-object]
---

Context: [context/viewer.md](../context/viewer.md).

Wearing an object from inventory marks the row "worn" immediately.
`inventory_actions.rs` and `inventory_drag.rs` insert the item into
`WornAttachments` when they send the rez-attachment command, and `is_worn`
also counts the item as worn once a COF link names it. That happens whether
or not the simulator actually attaches the object.

On 2026-09-27 OpenSim failed every attach (a SQLite constraint violation,
see [[parcel-properties-update-via-udp-poisons-opensim]]). The inventory
still showed `SLClientHudTouch` as worn, with no object on the HUD and
nothing in the scene. The user had no way to tell that the attach had
failed.

The reference's worn label comes from the avatar
(`get_is_item_worn` → `isWearingAttachment`), which is true only once an
attached object carrying that `AttachItemID` exists on the agent's own
avatar. A request, or a COF link, is not enough.

The other direction is broken the same way, also seen on 2026-09-27. Detaching
the HUD from its pie menu left the inventory row reading "(worn)". Only the
inventory's own Detach removes the item from `WornAttachments` and drops its
COF link. A detach from the attachment/HUD pie (`DetachObjects`), a script's
`llDetachFromAvatar`, an RLV-forced detach, or the simulator killing the
attachment changes neither. The reference's
`LLVOAvatarSelf::detachObject` → `LLAppearanceMgr::unregisterAttachment`
removes the COF link whenever the attachment object goes away, whoever caused
it.

Fix: for attachments, the worn state (label, Take Off / Detach, Wear
greyed, `FOLDER_HAS_WORN`) should follow the attachment objects actually on
our avatar (`ObjectState::worn_attachment_of_item`). A pending attach can be
shown as pending, but not as worn. When an attach never arrives, the row
should drop back to not worn, and the stray COF link should be cleaned up the
way the reference's COF reconciliation does.

Also drop the COF link (the reference's `unregisterAttachment`) whenever our
own attachment object leaves, whatever removed it.

Tests: an attach whose object never arrives does not read as worn, and a worn
attachment whose object is killed (a pie detach) reads as not worn and loses
its COF link.

## Done (2026-09-27)

An attachment's worn state is now the avatar's, as in the reference.

- **`WornAttachments` is derived, never written by an action.** Every frame,
  `sync_worn_attachments` rebuilds it from
  `ObjectState::inventory_attachments_worn_by`. That is every attachment root
  hanging off our own avatar object that names an inventory item. Temporary
  (script-attached) attachments are left out. The guessed inserts and removes
  in the wear, detach, outfit and drag-wear paths are gone, and so is
  `seed_worn_from_cof`.
- **`is_worn` asks only that set for an object.** A COF link no longer makes
  an attachment read as worn, since it can outlive a detach. Wearables still
  read the legacy set and the COF.
- **The COF follows the objects** (`AttachmentCofSync`, the reference's
  `LLAttachmentsMgr` / `unregisterAttachment`). An attachment is linked when
  its object arrives, once and only after the COF's contents have loaded. The
  wear request no longer writes the link, so a refused attach leaves no stale
  link behind. A departure drops the link only if, `DETACH_SETTLE_SECS` (5 s)
  later, the avatar object it hung off is still ours. A teleport, region
  crossing or logout removes that avatar object as well (`AvatarState` forgets
  it, or clears every scoped id on a region change), so those never unwear
  anything. A detach from anywhere does unwear: the attachment pie, a script,
  RLV, or the simulator.

Not done: a stale COF link left over from an earlier session, pointing at
an attachment that never arrives, is not cleaned up. It no longer makes the
row read as worn. The reference has no runtime cleanup for it either.

### What the teleport test found

After a few teleports, Detach stopped working from both the pie menu and the
inventory. Two bugs underneath, both fixed here:

- **OpenSim never kills what leaves for a neighbour we can see.**
  `ScenePresence.MakeChildAgent` sends the `KillObject` for the avatar and
  its attachments only to viewers that do not see the new region. Ours
  always does, so every neighbour teleport left a stale copy of our
  attachments behind: still drawn on the HUD, and still what the worn state
  and the pie's Detach reached. The session's `WorldCache` now indexes
  objects by full id (`by_full_id`, maintained by `insert_object` /
  `remove_object`). An object arriving on a circuit while another circuit
  caches the same full id has moved. The stale copy is dropped and announced
  as `ObjectRemoved`, after the new copy's `ObjectAdded`. This is the
  reference's full-id-keyed `LLViewerObjectList`, and it lives in `sl-proto`,
  so both runtimes and every viewer store get it.
- **An already-known avatar arriving under a new scoped id was never mapped
  to its agent.** `avatars.rs` `apply_object`'s existing-avatar branch
  returned without touching `by_scoped`, so the attachments the new region
  streamed hung off an avatar id that resolved to nobody. It now re-keys the
  avatar with `AvatarState::rekey_avatar`, moving the agent's one mapping to
  the new id. The superseded copy's `ObjectRemoved` then finds nothing
  mapped, so it cannot despawn the live avatar.

The session never asks a region to re-send a copy it superseded
(`WorldCache::supersede` / `is_superseded`). The region we left may keep
mentioning it until its own kill lands, through a terse update or a child
naming it as its parent. Re-sent, the stale copy would supersede the live
one in turn.

The fake grid's ridden-crossing fixture held the vehicle, and its other
rider, in **both** regions from the start, under one full id. No grid does
that, and the supersede rightly treated the neighbour's copy as the live
one. Now the destination starts without them and receives them at the
handover (`FakeAgent::receive_crossing`), before the source's kill, which is
the order a real destination sees.
