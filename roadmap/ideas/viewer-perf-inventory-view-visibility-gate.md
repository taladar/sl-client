---
id: viewer-perf-inventory-view-visibility-gate
title: Don't rebuild the inventory view while the floater is closed
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling, viewer-perf-run-condition-gating]
---

Context: [context/viewer.md](../context/viewer.md).

`rebuild_view` (`inventory.rs:1977`, registered `inventory.rs:173`) is
gated by change detection — `model.is_changed() || state.is_changed() ||
worn.is_changed() || filters.is_changed()` (`inventory.rs:1987`) — but
**not by visibility**. It calls `model.build_rows(...)`
(`inventory.rs:2004`): a full flatten + filter + sort of the expanded
inventory tree into display rows.

The model fold `ingest_inventory` (`inventory.rs:1755`) rightly runs
regardless of UI state — but during login, inventory folder/item pages
stream in continuously, and worn-attachment changes fire on every
clothing change. Each such model change triggers a full O(N) presentation
rebuild of a possibly 10k+ item tree **producing a row Vec nobody can
see**, floater closed or not. The cost concentrates exactly where frame
time is already tight: login streaming and appearance changes.

## Proposed fix

The specific, highest-value instance of
[[viewer-perf-run-condition-gating]]:

- Gate `rebuild_view` on the inventory panel being shown — a run
  condition on `UiPanelShown(true)` for `InventoryUi.panel` (or an
  equivalent internal early-return).
- Mark the view dirty instead while hidden, and force exactly one
  rebuild on the open transition — `refresh_inventory_on_show`
  (`inventory.rs:1720`, `Changed<UiPanelShown>`-gated) is the existing
  hook to extend, so the panel always opens up to date.
- Same gate for the small stuff riding the chain:
  `update_gear_conditions` (`inventory.rs:1498-1532`) allocates a Vec
  every frame while the panel is closed — gate it, and swap the Vec for
  a `SmallVec`/array while there.

## Estimated impact

Medium; scales with inventory size. For a 10k-item inventory, login
streaming currently triggers dozens-to-hundreds of full flatten passes
before the user ever opens the floater — all eliminated. Also removes
the rebuild from every outfit change while the floater is closed (the
common case). Verify with [[viewer-profiling]]: `build_rows` zone counts
during login should drop to zero with the floater closed, and to exactly
one on first open.

Confidence: medium-high — the change-only gate and `build_rows` call
verified; `build_rows`' absolute cost unprofiled (the count reduction is
certain regardless).
