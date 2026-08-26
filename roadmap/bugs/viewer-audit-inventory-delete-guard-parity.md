---
id: viewer-audit-inventory-delete-guard-parity
title: The keyboard delete path omits the in-trash check the menu path has
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-inventory/src/inventory_actions.rs` has two delete paths with two
different guards:

- the context-menu `"delete"` handler (`:1889`) re-checks nothing, trusting
  `CAN_DELETE` (built at `:759` / `:856`: not-library, `folder_type == None`,
  `!in_trash`) plus `visible_when(NOT_IN_TRASH)`;
- the keyboard Delete/Backspace path (`:2867-2905`) re-derives its own predicate
  — `folder_type == None && parent_id.is_some() && !is_library` — which **omits
  the in-trash check**.

So Delete on an already-trashed row re-sends a `MoveInventoryFolder` into Trash,
where the menu deliberately offers Purge instead.

`edit_undo.rs:44` states the intended discipline ("the shortcut path re-checks
before sending"). Fix: one `can_delete(...)` predicate called by both. The menu
half already has tests (`:3500-3687`); they simply do not reach the keyboard
path.
