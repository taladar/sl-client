---
id: viewer-audit-inventory-merge-and-tree-walks
title: The chunked inventory merge is quadratic, and the downward tree walks have no cycle guard
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

Two defects in `sl-viewer-inventory/src/inventory.rs`, both now fixed.

**The chunking was defeated by a full reindex.** `merge_folders` called
`reindex()`, which cleared and rebuilt the *whole* `child_folders` index and
re-sorted every child list. `drain_skeleton_merge` calls it once per
1000-folder chunk (`DEFAULT_INVENTORY_MERGE_BUDGET`), so a 20k-folder skeleton
did 20 full O(n log n) reindexes instead of one — the chunking bought repeats,
not less work. Worse, the sort keys were `info.name.to_lowercase()` inside
`sort_by_key`, which rebuilds the key on **every comparison**.

`merge_folders` now maintains the index for the batch alone: each folder is
unlinked from the list it sat in only when it is new or re-parented, pushed
into the list its parent names, and the lists the batch actually touched are
re-sorted once at the end with `sort_by_cached_key`. `reindex` is gone.
`set_items` and the three sorts in
`sl-viewer-pickers/src/ui_texture_picker.rs` use cached keys too.

**The downward walks could stack-overflow.** `is_within` was explicitly
bounded against "a (server-side impossible) parent cycle", but `emit_folder`,
`mark_matching_subtree`, `emit_filtered_folder`, `mark_member_subtree`,
`emit_member_folder`, `collect_subtree_items`, the picker's own `emit_folder`
and `deep_copy_folder` all recursed on `children_of` with no visited set and no
depth cap — and `subtree_folders`, being iterative, would have grown its queue
forever instead. `mark_matching_subtree` is not gated on `expanded`, so a
parent cycle reachable from a root was an unconditional stack overflow **the
moment a search query was typed**.

Defended at the source rather than in each walk: `merge_folders` is the one
place the tree grows an edge, so `link` now **refuses a parent edge that would
close a cycle** (asking the bounded upward walk) and lifts the folder to a root
instead. An index that cannot hold a cycle makes every downward walk finite.
Behind that, each walk carries `MAX_FOLDER_DEPTH` (64 — the bound `is_within`
already used) as a backstop, and `subtree_folders` enqueues each folder once.
