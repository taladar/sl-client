---
id: viewer-audit-inventory-delete-guard-parity
title: The keyboard delete path omits the in-trash check the menu path has
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-inventory/src/inventory_actions.rs` had two delete paths with two
different guards:

- the context-menu `"delete"` handler trusted `CAN_DELETE`, built from
  not-library, `folder_type == None` and `!in_trash`, plus
  `visible_when(NOT_IN_TRASH)`;
- the keyboard Delete / Backspace path re-derived its own predicate —
  `folder_type == None && parent_id.is_some() && !is_library` — which **omitted
  the in-trash check**.

So Delete on an already-trashed row re-sent a `MoveInventoryFolder` (or
`MoveInventoryItem`) into the Trash, where the menu deliberately offers Purge
instead: a move that is a no-op the grid still has to answer, and a second
`QueryInventoryFolders` behind it.

Now one predicate, asked by both. `item_can_delete` / `folder_can_delete` take
the same `ItemMenuFacts` / `FolderMenuFacts` the menu conditions are built from,
and `item_conditions` / `folder_conditions` push `CAN_DELETE` from them rather
than re-deriving it inline. The facts themselves come from `item_delete_facts` /
`folder_delete_facts`, which read library and Trash placement out of the model
(`within_trash`), so `resolve_row_target` spreads them into the full fact set
and the shortcut — which has no menu conditions to consult — asks
`can_delete_row`, the row-key form of the same thing.

The folder half gained the shortcut's `parent_id.is_some()` guard in the
bargain: the menu's own predicate had never had it, so the union is what both
now enforce.

`both_delete_paths_agree_on_every_row` walks a tree — a live item and folder, an
item and two folders under the Trash, the Trash itself, the root, and a row that
does not resolve — and asserts for each that `can_delete_row` and the conditions
`resolve_row_target` actually builds give the *same* answer. The existing menu
tests never reached the keyboard path; this one pins the two together.
