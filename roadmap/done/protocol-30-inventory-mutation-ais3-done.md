---
id: protocol-30
title: Inventory mutation & AIS3 (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**30. Inventory mutation & AIS3 (done) ✅ — `CreateInventoryFolder`/`Item`,
`MoveInventory*`, `CopyInventoryItem`, `RemoveInventoryItem`/`Objects`,
`UpdateInventoryItem`, `ChangeInventoryItemFlags`, `PurgeInventoryDescendents`,
`BulkUpdateInventory`/`UpdateCreateInventoryItem`, `CreateInventoryCategory` +
`InventoryAPIv3` · 8 pts. (extends #5, Tier A.)** Item #5 delivered the fetch
tree over both UDP and CAPS but deferred all mutation; this adds the full write
surface plus a **live inventory cache**. **Cache:** `Session` now keeps a
folder/item cache (`inventory_folder`/`inventory_item`/`inventory_folders`/
`inventory_items`/`inventory_children`), seeded from the login skeleton, grown
by descendents fetches (both transports), kept current by the simulator's
`BulkUpdateInventory`/`UpdateCreateInventoryItem` pushes — decoded as
`Event::InventoryBulkUpdate` / `Event::InventoryItemCreated` over both the UDP
packets and the CAPS event-queue `BulkUpdateInventory` — and updated
optimistically by the agent's own mutations. **UDP mutation:**
`create_inventory_folder`, `update_inventory_folder`,
`move_inventory_folder(s)`, `remove_inventory_folders`, `create_inventory_item`
(→ `UpdateCreateInventoryItem` with the echoed `CallbackID`),
`update_inventory_item` (with a faithful `UpdateInventoryItem` **CRC** — a port
of the viewer's `LLInventoryItem::getCRC32`, so SL's checksum matches; this
added a `last_owner_id` field to `InventoryItem`, populated from the CAPS/AIS
permissions map), `move_inventory_item(s)`, `copy_inventory_item`,
`remove_inventory_items`, `change_inventory_item_flags`,
`purge_inventory_descendents`, `remove_inventory_objects`, all wired as
`Command`/`SlCommand` variants through both runtimes. **CAPS:** the
`CreateInventoryCategory` cap (served by **both** OpenSim and Second Life) gives
a *confirmed* folder create (a synchronous
`{ folder_id, name, parent_id, type }` reply), and the modern **AIS3**
(`InventoryAPIv3`/`LibraryAPIv3`) REST surface —
`POST /category/<parent>?tid=`, `PATCH`/`DELETE /category/<id>` and
`/item/<id>`, `GET …/children?depth=` — is built in a new
`sl-wire/src/inventory.rs` (URL + LLSD-body builders) and driven by `Ais3*`
runtime commands (a new `patch_caps_llsd` verb), with replies decoded into
`Event::InventoryBulkUpdate` (the `_embedded` categories/items). New value type
`NewInventoryItem`; three caps (`InventoryAPIv3`, `LibraryAPIv3`,
`CreateInventoryCategory`) join the seed. Covered by five `sl-wire` unit tests
(the AIS URL/body builders + `CreateInventoryCategory` body) and six `sl-proto`
lifecycle tests (the create-folder / create-item / update-item-golden-CRC /
move-item encodings, and the `UpdateCreateInventoryItem` + `BulkUpdateInventory`
inbound decode + cache). *Live-verified against the local OpenSim via the new
`inventory_edit` tokio example: logged in (20-folder skeleton, root learned),
the `CreateInventoryCategory` cap returned a confirmed new folder
(`InventoryBulkUpdate`), a `CreateInventoryItem` round-tripped its
`UpdateCreateInventoryItem` (`InventoryItemCreated`), then the item was renamed
(`UpdateInventoryItem`) and the item + folder removed — a clean create → update
→ delete cycle on one login.* **AIS3 is Second-Life only** — stock OpenSim
serves no `InventoryAPIv3` cap, so the `Ais3*` commands no-op there and are
unit-tested only (as with #20/#26/#27's SL-only caps); the UDP mutation, cache,
and `CreateInventoryCategory` paths are the OpenSim-testable ones.
*Test: local OpenSim.*
