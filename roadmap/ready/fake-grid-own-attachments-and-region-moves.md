---
id: fake-grid-own-attachments-and-region-moves
title: Fake grid — the agent's own attachments, the COF, and region moves
topic: test
status: ready
origin: gap found while fixing viewer-inventory-worn-before-attach-confirmed (2026-09-27)
refs: [viewer-inventory-worn-before-attach-confirmed, viewer-object-moved-region-reuse, viewer-menu-touch-object]
---

Context: [context/test.md](../context/test.md).

Everything [[viewer-inventory-worn-before-attach-confirmed]] and
[[viewer-menu-touch-object]] fixed was only reproducible on the live
OpenSim, because `sl-fake-grid` models none of it. The survey of
2026-09-27:

- **Wearing and detaching from inventory is decoded and ignored.**
  `SimSession` turns `RezSingleAttachmentFromInv` /
  `RezMultipleAttachmentsFromInv`, `ObjectAttach`, `ObjectDetach` and
  `DetachAttachmentIntoInv` into server events, and `world.rs`'s catch-all
  drops every one. There is no attach at login from the appearance either.
  Only timeline actions (`Action::Attach` / `Detach`) and NPC fixtures
  (`PrimFixture::attached_to`) put an attachment on an avatar.
- **The COF is seeded but never written.** Body-part links exist at login.
  `LinkInventoryItem` is decoded and ignored, and `RemoveInventoryItem` /
  `RemoveInventoryObjects` are not decoded server-side at all, so the
  viewer's COF maintenance goes nowhere.
- **Region moves do not carry attachments.** A crossing or neighbour
  teleport kills nothing on the old circuit, which matches OpenSim's
  `MakeChildAgent`. But the destination rezzes only the avatar, not its
  attachments. Every region also gives the avatar local id 1, where real
  grids assign each region its own, so the "same full id, new local id"
  move is never exercised.
- **`OwnerID` is always the real owner.** OpenSim sends it zeroed unless the
  object has an attached sound (it is the sound's owner). The viewer's first
  inventory-Touch lookup matched on it, and it passed everything the fake
  grid could throw at it.

## Scope

- Handle the attach events: rez the item's object on the agent's avatar
  with its attachment-point `state`, `AttachItemID` and parent, replacing or
  adding per the mode. A refused attach (a missing item, or a scenario
  switch) sends nothing, so the "requested but never arrived" case is
  testable. Re-attach the appearance's attachments at login.
- Handle the detach events: `KillObject` for the attachment, and the item
  back in inventory.
- Accept COF writes: `LinkInventoryItem` into the COF and
  `RemoveInventoryItem` / `RemoveInventoryObjects` out of it, with the
  folder version bumped so the viewer's refetch sees them.
- Carry the avatar's attachments to the destination of a crossing or
  neighbour teleport under the same full ids and **new** local ids. Give
  each region its own avatar local id. Make the old region's kill a
  scenario switch: none (OpenSim), or kills before the arrival (Second Life,
  the order [[viewer-object-moved-region-reuse]] has to survive).
- An `OwnerID` mode matching OpenSim (zeroed without a sound), defaulting to
  it.

## Verify

End-to-end tests against the unmodified client: wear a HUD from inventory
and see it arrive with its COF link; detach it from the pie menu and from
inventory and see the kill and the link drop; teleport to a neighbour and
back with it worn, and see one copy, still worn, still linked; a refused
attach never reads as worn.
