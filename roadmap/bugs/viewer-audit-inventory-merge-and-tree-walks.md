---
id: viewer-audit-inventory-merge-and-tree-walks
title: The chunked inventory merge is quadratic, and the downward tree walks have no cycle guard
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

Two defects in `sl-viewer-inventory/src/inventory.rs`:

**The chunking is defeated by a full reindex.** `merge_folders` (`:445`) calls
`self.reindex()`, which clears and rebuilds the *whole* `child_folders` index
and re-sorts every child list (`:478-511`). `drain_skeleton_merge` (`:2025`)
calls it once per 1000-folder chunk (`DEFAULT_INVENTORY_MERGE_BUDGET`, `:1964`),
so a 20k-folder skeleton does 20 full O(n log n) reindexes instead of one.
Worse, the sort keys are `info.name.to_lowercase()` (`:498`, `:507`) inside
`sort_by_key`, which recomputes the key on **every comparison** — roughly 5.7M
`String` allocations for that login. `sort_by_cached_key` is a one-word fix, and
the same applies at `:517` and `sl-viewer-pickers/src/ui_texture_picker.rs:763`
and `:814`.

**The downward walks can stack-overflow.** `is_within` (`:319-335`) is
explicitly bounded `for _step in 0..64` against "a (server-side impossible)
parent cycle" — but `emit_folder` (`:688`), `mark_matching_subtree` (`:762`),
`emit_filtered_folder` (`:794`) and `mark_member_subtree` recurse on
`children_of` with no visited set and no depth cap. `mark_matching_subtree` is
not even gated on `expanded`, so a parent cycle reachable from a root is an
unconditional stack overflow **the moment a search query is typed**. Same
hazard, defended in one direction only.
