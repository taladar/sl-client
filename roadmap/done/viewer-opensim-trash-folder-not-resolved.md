---
id: viewer-opensim-trash-folder-not-resolved
title: OpenSim — object-pie Delete does nothing (Trash system folder never resolved)
topic: viewer
status: done
origin: user report (2026-07-26) while testing the Create tool on the local
  OpenSim; Delete works on aditi (SL) but not OpenSim
refs: [viewer-object-context-menu, viewer-inventory-folder-tree]
---

Context: [context/viewer.md](../context/viewer.md).

Right-clicking an in-world object and picking **Delete** does nothing on the
local OpenSim grid. The log shows, on every attempt:

```text
WARN sl_client_bevy_viewer::object_menu: object menu: no Trash folder known
yet; ignoring UiAction { element: "object-menu", action: "delete" }
```

The Delete path (`object_menu.rs`) derezzes the picked linkset root into the
**Trash** system folder, resolving it with
`InventoryModel::folder_by_type(FolderType::Trash)`; when that returns `None`
the action is skipped rather than derezzing into nowhere. So the object is never
deleted. **Take** / **Take Copy** (which resolve `FolderType::Object`) may hit
the same gap.

The same Delete works on **aditi (SL)**, where the Trash folder is present and
correctly typed — so this is an OpenSim inventory-skeleton difference, not a
regression in the object pie. Likely causes to investigate:

- the agent inventory skeleton is not fetched (or not fetched yet) at the time
  Delete runs, so `folders` holds no Trash-typed folder — a timing / fetch gap;
- OpenSim reports the Trash folder with a different `type_default` /
  preferred-type than the viewer maps to `FolderType::Trash`, so the folder is
  present but untyped in the model — a folder-type-decode gap.

Confirm which by dumping the agent folder set + their `folder_type`s after login
on OpenSim, and compare against aditi. Fix is either to ensure the system
folders are fetched/typed on OpenSim, or (fallback) to create/adopt a Trash
folder when the grid does not advertise one, matching the reference viewer's
`gInventory.findCategoryUUIDForType(FT_TRASH)` which falls back to creating it.

Reference (Firestorm, read-only): `llinventorymodel.cpp`
(`findCategoryUUIDForType` / `findCategoryUUIDForTypeInRoot`, and the
create-on-missing fallback), `llviewermessage.cpp` derez handling.

## Done

Root cause was a **query-timing gap**, not a folder-type decode: OpenSim *does*
advertise a correctly-typed Trash folder (`type_default = 14`) in the login
skeleton (`XInventoryService.CreateUserInventory` → `GetInventorySkeleton`), and
`FolderType::from_code(14)` maps it to `Trash`. But nothing populated the
viewer's `InventoryModel` at login on OpenSim: the model's folders are snapshot
(via `Command::QueryInventoryFolders`) only when the inventory window is opened
(`refresh_inventory_on_show`) **or** when the central-baking capability appears
(`appearance.rs` `drive_server_bake`). Second Life offers that capability, so
its folders load at login and Delete finds the Trash; OpenSim offers no such
capability, so the model stayed empty until the window was opened — and the
object pie's Delete / Take (which read `folder_by_type` in `object_menu.rs`)
found no Trash / Objects folder.

Fix (`inventory.rs` `ingest_inventory`): on the grid-agnostic login
`InventorySkeleton` event — which fires exactly once at login — issue a one-shot
`Command::QueryInventoryFolders` (guarded by `folders_loaded`, so a re-bake's
repeated skeleton does not restart the first-load expansion). Routing through
the existing `InventoryFolders` handler reuses the first-load root expansion, so
every grid now behaves the way Second Life already did. Since OpenSim genuinely
advertises a Trash folder, the reference `findCategoryUUIDForType`
create-on-missing fallback was **not** needed and was left unimplemented (it
would be unexercised code).

Client-side tests (`inventory.rs`): `folder_by_type` resolves the agent Trash
over a same-typed Library folder; the login skeleton event writes the one-shot
folder query; a second skeleton after load does not re-query.
