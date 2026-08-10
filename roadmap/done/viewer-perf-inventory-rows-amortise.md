---
id: viewer-perf-inventory-rows-amortise
title: Amortise inventory row rebuilds and the skeleton merge
topic: viewer
status: done
origin: unbounded-frame-work survey (2026-08-09, performance branch)
refs: []
---

Context: [context/viewer.md](../context/viewer.md).

Two single-frame inventory bursts, both O(entire inventory):

- **Search-as-you-type re-flatten.** The inventory UI is virtualised (only
  visible rows spawn), but `InventoryModel::build_rows` re-derives the full
  flat row `Vec` on every model/state/filter change — including every
  keystroke in the search field. Fix: `rebuild_view` debounces a state
  change that only moved the query text by 0.15 s of typing quiet (a
  deferral folds into any other rebuild trigger, which always reads the
  newest query); tab / sort / filter / model changes stay immediate.
- **Login skeleton merge.** `ingest_inventory` merged the whole
  `InventoryFolders` skeleton (thousands of folders on a big inventory) in
  one call. The event's `Arc<[FolderInfo]>` now parks in
  `PendingSkeletonMerge` and `drain_skeleton_merge` (chained right after
  ingest, so a small skeleton still lands the same frame) merges
  `SL_VIEWER_INVENTORY_MERGE_BUDGET` (default 1000) folders per frame. The
  one-shot first-load work (COF prefetch, agent-root expansion) runs once
  the backlog empties; `folders_loaded` latches at ingest so a repeat
  skeleton (re-bake) cannot re-queue it and the raw `InventorySkeleton`
  fallback stays suppressed.

Checked and deliberately not touched: the other virtual-list floaters
(people, groups, search, group members) re-derive their row `Vec`s on
change too, but they are revision-gated and their data sets are orders of
magnitude smaller than an inventory — debounce them only if profiling ever
shows it.

The plan's `Arc`-snapshot off-thread `build_rows` remains a follow-up only
if Tracy still shows spikes post-debounce (a deep model clone would rival
the rebuild cost).
