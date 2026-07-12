---
id: missing-batch-6
title: inventory sync, task inventory & misc
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

## Batch 6 — inventory sync, task inventory & misc

Server-initiated inventory mutations to keep a client mirror current:
`RemoveInventoryItem` (270), `RemoveInventoryFolder` (276),
`RemoveInventoryObjects` (284), `MoveInventoryItem` (268). Plus
`ReplyTaskInventory` (290, object contents), `UserInfoReply` (400, email/IM
prefs), `DeRezAck` (292), `ForceObjectSelect` (205), `GrantGodlikePowers` (258).

Implemented as nine `Event` variants (the echoed `AgentData.AgentID` is dropped
on every one — it is just this agent's own id):

- `RemoveInventoryItem` → `Event::InventoryItemsRemoved { items:
  Vec<InventoryKey> }`; `RemoveInventoryFolder` →
  `Event::InventoryFoldersRemoved { folders: Vec<InventoryFolderKey> }`;
  `RemoveInventoryObjects` → `Event::InventoryObjectsRemoved { folders, items }`
  (mixed folders + items in one message).
- `MoveInventoryItem` → `Event::InventoryItemsMoved { stamp: bool, moves:
  Vec<InventoryItemMove> }`, where `InventoryItemMove { item: InventoryKey,
  folder: InventoryFolderKey, new_name: Option<String> }` (in
  `types/inventory.rs`; an empty wire `NewName` maps to `None`) and `stamp`
  echoes the re-timestamp flag.
- `ReplyTaskInventory` → `Event::TaskInventoryReply(TaskInventoryReply { task:
  ObjectKey, serial: i16, filename: String })` (in `types/object.rs`); the
  filename is the temporary Xfer file the full contents listing is downloaded
  from.
- `UserInfoReply` → `Event::UserInfo(UserInfo { im_via_email: bool,
  directory_visibility: String, email: String })` (in
  `types/avatar_profile.rs`).
- `DeRezAck` → `Event::DeRezAck { transaction: TransactionId, success: bool }`.
- `ForceObjectSelect` → `Event::ForceObjectSelect { reset_list: bool, objects:
  Vec<ScopedObjectId> }`; the region-local ids are scoped to the originating
  circuit (skipped if the circuit is unknown).
- `GrantGodlikePowers` → `Event::GodlikePowersGranted { god_level: u8 }`; the
  wire `Token` is checked on the sim and ignored by the viewer, so it is
  dropped.
