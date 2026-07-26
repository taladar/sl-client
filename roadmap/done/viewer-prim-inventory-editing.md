---
id: viewer-prim-inventory-editing
title: Prim inventory (contents) editing
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-edit-floater-shell, viewer-ui-virtualized-list]
---

Context: [context/viewer.md](../context/viewer.md).

The object **contents** tab of the edit floater
([[viewer-object-edit-floater-shell]]): list the items inside a prim (a
virtualized list, [[viewer-ui-virtualized-list]]), add (drag-in), remove, and
rename them, and drop scripts / notecards into an object.

Include the "Save Back to Object Contents" quick-save action
(`Tools.SaveToObjectInventory`, Build ▸ Object): write an item opened from
an object's contents straight back into that object (main-menu survey
2026-07-23).

Reference (Firestorm, read-only): `llpanelcontents`, `llsidepaneltaskinfo`;
messages `RequestTaskInventory`, `UpdateTaskInventory`, `RemoveTaskInventory`.

## Done

`sl-client-bevy-viewer/src/edit_contents.rs` — the **Content tab** of the Build
Tools floater and a standalone **Object Contents** floater (the reference's
`LLFloaterOpenObject`, opened by the object pie's **Open** action). Both share a
virtualized list, the per-object cache, and the row machinery, tagged by
`ContentsSurface`.

- **Per-prim cache** (`TaskInventoryCache`, keyed by each prim's own
  `ObjectKey`, session-lifetime): fetched once via
  `Command::FetchTaskInventory`; cycling the linked-part nav re-shows an
  already-loaded prim's contents with no wire traffic. The Content tab follows
  the primary selection, so in edit-linked mode it shows the selected member
  prim, not the root.
- **Server-authoritative reconcile with per-item pending tracking**: no
  optimistic cache mutation — every edit re-fetches and the authoritative
  listing is the source of truth, so nothing drifts and a rejected edit visibly
  reverts. A `PendingMutations` overlay greys the touched row with
  **…adding / …deleting / …refreshing** and blocks re-editing *that* item, while
  untouched items stay editable (batching).
- **Actions**: New Script, inline Rename (item-level modify gate), Remove,
  Refresh; **drag-in add** onto the list *and* onto the in-world object
  (Ctrl-drag for an object item, per the reference's `dad3dUpdateInventory`),
  with the reference's hover outline on the target — green when you may edit it,
  **red** for a foreign object that accepts the drop via its "allow anyone to
  add inventory" flag. Copy To Inventory / Copy And Wear on the Open floater.
- **Permissions** distinguish object vs. content perms exactly as the reference:
  add needs object-modify *or* the allow-inventory-drop flag; remove is offered
  on modify-or-own but only applied with modify; rename is offered on
  modify-or-own but only applied when the *item* itself is modifiable.
- **Keyboard**: **F2** renames / **Delete** (+ **Backspace** in a list) removes,
  focus-routed so the same keys hit the right target — the Content tab list, the
  regular inventory list, or (build-mode, world focused) the selected in-world
  objects. The three handlers are kept apart by `InputContext` + a
  `focus_within` gate, so a focused list keeps `Delete` for its own selection.
- Protocol helpers `RestoreItem::from_task_item` (rename, CRC-preserving) and
  `ItemInfo::to_item` (drag-in) in `sl-proto`; i18n in all four locales; unit
  tests for the perm gates, item-modify, the pending-overlay merge, and the
  rename rebuild.

Deferred to follow-ups (needs the asset editors, which are separate blocked
tasks): the "Save Back to Object Contents" quick-save and opening an item from
contents into its editor ([[viewer-task-inventory-open-and-save-back]]); the
Open floater's named copy folder and Copy-And-Wear auto-wear
([[viewer-object-contents-copy-refinements]]).
