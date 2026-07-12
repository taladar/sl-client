---
id: protocol-44
title: Inventory push fidelity (extends #30, Tier A). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**44. Inventory push fidelity (extends #30, Tier A). ✅ Done.**
`UpdateCreateInventoryItem` (`session.rs`) used `.first()` on the repeatable
`InventoryData` block, so when the simulator batched more than one created item
into one message all but the first were dropped (from both the event and the
cache); and `bulk_update_item` dropped the per-item `CallbackID`, breaking
create-callback correlation when a result arrives as a `BulkUpdateInventory`
rather than an `UpdateCreateInventoryItem`. Fixed both: the handler now iterates
**every** `InventoryData` entry — caching each item and emitting an
[`Event::InventoryItemCreated`] per entry — and the `BulkUpdateInventory` path
collects each item's non-zero `CallbackID` into a new
**`InventoryBulkUpdate::item_callbacks: Vec<(Uuid, u32)>`** field (`(item_id,
callback_id)` pairs), so a client that issued a `copy_inventory_item` /
`create_inventory_item` (each returning a callback id) can correlate the result
to the resulting item id even when it lands as a bulk update. The three CAPS
delivery paths (event-queue `BulkUpdateInventory`, AIS3, category-create), which
carry no callback id, pass an empty `item_callbacks`. The new field flows
unchanged through both runtimes (events pass through), and the
`inventory_edit` example now copies the item it creates and logs any surfaced
callback correlation. Covered by the extended `update_create_inventory_item_*`
(two batched `InventoryData` entries → two cached items + two events) and
`bulk_update_inventory_*` (a non-zero `CallbackID` round-trips into
`item_callbacks`) `sl-proto` lifecycle tests. *Live-verified against the local
OpenSim via `inventory_edit`: create → rename → **copy** → remove ran with no
protocol error and both the original create and the copy surfaced as
`InventoryItemCreated` (a live finding: OpenSim answers `CopyInventoryItem` with
`UpdateCreateInventoryItem`, not the `BulkUpdateInventory` Second Life sends, so
the bulk-callback path is exercised by the deterministic lifecycle test rather
than this grid). Test: local OpenSim.*
