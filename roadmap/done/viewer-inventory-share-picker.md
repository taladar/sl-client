---
id: viewer-inventory-share-picker
title: Share (give via avatar picker)
topic: viewer
status: done
origin: split from viewer-inventory-context-actions (2026-07-21) — shipped
  greyed (UNIMPLEMENTED)
refs: [viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

The context menu's **"Share"**: give the item / folder to an avatar chosen
from a **picker** (search by name, pick from friends / nearby), for when the
recipient is not conveniently on screen. The give wire path and the
drag-to-give (onto an avatar in-world, a name tag, or a People-list row) are
live (`inventory_drag.rs`, `Command::GiveInventory` /
`GiveInventoryFolder`); this task is the avatar-picker dialog and its wiring —
a reusable picker other features (pay, invite-to-group, teleport offers) will
want too.

Reference (Firestorm, read-only): `llavatarpicker` / `llfloateravatarpicker`,
`llinventoryfunctions.cpp` (`LLShareInfo` / the Share flow).

Shipped 2026-07-22: a reusable `avatar_picker` module (requester-tagged
open/pick messages) with the reference's Search (AvatarPickerRequest
UDP) / Friends / Near Me sources; Near Me lists every known avatar
nearest-first instead of the radius slider. The inventory Share entries
go live through it (gated `can-share`: own inventory, transferable
items). Pay / group invites / teleport offers can reuse the picker.
