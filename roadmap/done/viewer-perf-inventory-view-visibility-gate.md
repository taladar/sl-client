---
id: viewer-perf-inventory-view-visibility-gate
title: Don't rebuild the inventory view while the floater is closed
topic: viewer
status: done
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

> 2026-08-10 (the amortisation pass,
> [[viewer-perf-inventory-rows-amortise]]): `rebuild_view` now debounces
> query-text-only changes by 0.15 s, so the per-keystroke rebuilds are
> gone — but every *model* change still rebuilds immediately, so this
> task's login-streaming case stands. It got slightly **more** relevant:
> the chunked login-skeleton merge marks the model changed once per
> drained chunk (one `build_rows` pass per merge frame). When adding the
> visibility gate, keep it compatible with the debounce locals (mark
> dirty while hidden; one forced rebuild on the open transition clears
> both). Line references predate the pass — anchor on the function names.

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

## Done (2026-08-10)

Implemented exactly as proposed, via a new reusable run-condition builder
`floater_shown(id)` in `floater.rs` (keyed on the stable `Floater::id` +
`UiPanelShown`, next to `floater_panel`):

- `rebuild_view` and `update_gear_conditions` are registered
  `.run_if(floater_shown(INVENTORY_FLOATER_ID))` — neither runs while the
  floater is closed (or before its first spawn), so login streaming and
  outfit changes no longer trigger hidden `build_rows` flattens.
- No explicit dirty flag was needed: a `run_if`-skipped system keeps its
  change-detection ticks, so model/worn/filter changes accumulate while
  hidden and fold into one catch-up rebuild on the open transition. That
  rebuild is guaranteed: `refresh_inventory_on_show` (ungated, earlier in
  the same chain) takes `ResMut<InventoryModel>` on every open, marking
  the model changed.
- The debounce locals persist across skipped frames unchanged; a deferral
  pending at close ripens on the next real run.
- `update_gear_conditions` now also rebuilds its wanted-conditions list
  into a reused `Local<Vec<&'static str>>` scratch buffer (zero
  steady-state allocation; `SmallVec` rejected — not a direct workspace
  dependency, and the list holds at most five entries).

Deliberately not gated: `toggle_inventory` / `refresh_inventory_on_show`
(the open path), `ingest_inventory` / `drain_skeleton_merge` (model fold +
cross-frame backlog), `bridge_tab_selection` / `route_gear_menu` /
`apply_ui_actions` / `read_search_field` (consumers of 2-frame-expiry
messages), `apply_pending_reveal` (a reveal request must survive until the
panel opens), and everything after `layout_virtual_lists` (already free
via the virtual list's zero-viewport early-out). External
`InventoryView` readers (drag observers, row context menu, hotkeys) are
all interaction-driven on visible rows, so a stale view while hidden is
unreachable.
