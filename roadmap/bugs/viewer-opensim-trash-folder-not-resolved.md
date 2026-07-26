---
id: viewer-opensim-trash-folder-not-resolved
title: OpenSim — object-pie Delete does nothing (Trash system folder never resolved)
topic: viewer
status: bugs
origin: user report (2026-07-26) while testing the Create tool on the local
  OpenSim; Delete works on aditi (SL) but not OpenSim
refs: [viewer-object-context-menu, viewer-inventory]
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
