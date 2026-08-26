---
id: viewer-audit-table-sort-consolidation
title: The multi-column sort comparator is hand-written six times, and People reimplements TableSortState
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/viewer.md](../context/viewer.md).

The same comparator loop —
`for key in keys { let base = column_ordering(...); let ord = if key.ascending {
base } else { base.reverse() }; if ord != Ordering::Equal { return ord } }` plus
a tie-break — recurs at `sl-viewer-people/src/group_profile.rs:1435`
(`compare_members`), `:1532` (`compare_notices`), `:1905` (`compare_roles`),
`blocked.rs:259`, `contact_sets_panel.rs:242` and `avatar_profile.rs:1398`.

Separately, `sl-viewer-people/src/people.rs:620-800` defines its own
`SortColumn`, `SortKey`, `MAX_SORT_KEYS`, `SortState::{click, compare, encode,
parse}`, `parse_column_token` and a `FRIENDS_SORT_SETTING` persistence path —
about 180 lines duplicating `ui_table.rs:244 TableSortKey`, `:266-370
TableSortState::{from_defaults, click, keys, encode, parse}` and `sort_setting`.
The table is explicitly configured `builtin_sort: false` (`ui_table.rs:186-191`,
"the People friends list keeps its bespoke 8-way sort") to make room for it.

Scope: add `TableSortState::order_by(&mut rows, |column, l, r| Ordering)` to
`ui_table` — it already hands out `keys()` for exactly this — which kills all
six loops; then express People's eight columns as `TableColumn::token`s and
delete `SortState` in favour of `builtin_sort: true`.

`apply_persisted_widths` / `encode_widths` (`ui_table.rs:1315`, `:1339`) are a
string round-trip with clamping and no test — worth pinning in the same change.
