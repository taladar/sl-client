---
id: missing-out-batch-3
title: rez & script permissions
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 3 — rez & script permissions.** `RezObject` / `RezScript` (rez an
inventory object/script into the world), `RevokePermissions` (revoke
previously-granted script permissions), `DetachAttachmentIntoInv` (detach a
worn attachment back to inventory).

Implemented as `Session::rez_object_from_inventory(params: &RezObjectParams)`,
`Session::rez_script(target: ScopedObjectId, params: &RezScriptParams)`,
`Session::revoke_script_permissions(object_id: ObjectKey, permissions: ScriptPermissions)`,
and `Session::detach_attachment_into_inventory(item_id: InventoryKey)`. The
`RezObject` wire message rezzes an inventory item into the world (distinct from
the existing `Session::rez_object`, which builds a *new* prim via `ObjectAdd` —
hence the `_from_inventory` suffix to avoid the collision). New domain structs
`RezObjectParams` (ray placement + the per-object permission masks the rez
applies) and `RezScriptParams` (running flag + active group) both carry the
message's full inventory-item block as a reused `RestoreItem` — the same
per-item payload `RezRestoreToWorld` takes — rather than 20 raw wire fields.
`revoke_script_permissions` reuses the typed `ScriptPermissions` bitfield (the
inverse of `answer_script_permissions`); `detach_attachment_into_inventory` keys
off the worn item's `InventoryKey`, unlike `detach_objects` which needs the
rezzed object's region-local id. Wired as
`Command::{RezObjectFromInventory, RezScript, RevokeScriptPermissions, DetachAttachmentIntoInventory}`
through the tokio and bevy runtimes, the `command_name` formatter, and the
matching REPL tokens (`rez_object_from_inventory` / `rez_script` reuse a new
`restore_item_from_args` helper, which also de-duplicates the
`rez_restore_to_world` token's 20-field item builder). Covered by one
pack-the-wire lifecycle test and four REPL parse tests. Live-testable on OpenSim
(rez/detach/script-drop all work against the local grid).
