---
id: fake-grid-own-attachments-and-region-moves
title: Fake grid — the agent's own attachments, the COF, and region moves
topic: test
status: blocked
origin: gap found while fixing viewer-inventory-worn-before-attach-confirmed (2026-09-27)
refs: [viewer-inventory-worn-before-attach-confirmed, viewer-object-moved-region-reuse, viewer-menu-touch-object]
blocked_by: [gridspec-outfits-wearables, gridspec-attachments]
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

## Measured (gridspec-attachments, 2026-10-20)

`book/src/gridspec/appearance.md` § Attachments has the model to build to, and
the `attachments` census (live only for now) is the conformance case to put on
both offline flavours when this lands. What the scope above misses or has to
change:

- An attach kills nothing in the world: `ObjectAttach` (and a script's
  `llAttachToAvatar[Temp]`) sends the same object again, attached, under the
  same ids. A drop is the same in reverse, and removes the item.
- `OwnerID` is nil on an attachment and a dropped object on **both** grids;
  Second Life gives a viewer that is not the owner `OBJECT_OWNER_MODIFY`
  (`0x10000000`), OpenSim does not.
- *Replace* takes off one attachment of the point on Second Life, all of them
  on OpenSim. A worn item worn again, or an unknown item, is ignored; Second
  Life alerts `RezAttemptFailed` for the unknown one.
- The inventory messages: an `ObjectDetach` restates the item (event-queue
  `BulkUpdateInventory` on Second Life, UDP `UpdateCreateInventoryItem` on
  OpenSim); `DetachAttachmentIntoInv` restates nothing; an attach from the
  world announces a new item over UDP on both.
- A HUD goes to its wearer only; Second Life still sends other viewers the
  kill when it comes off.
- The limit is 38, the 39th refused silently.
- Across a teleport the attachment keeps its full id under a new local id.
  Second Life's region kills the avatar (for the agent and for viewers left
  behind) and never its attachments; OpenSim's kills nothing. Both grids
  re-wear what was worn at the next login.

## Verify

End-to-end tests against the unmodified client: wear a HUD from inventory
and see it arrive with its COF link; detach it from the pie menu and from
inventory and see the kill and the link drop; teleport to a neighbour and
back with it worn, and see one copy, still worn, still linked; a refused
attach never reads as worn.
