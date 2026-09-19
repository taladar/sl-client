---
id: viewer-audit-table-sort-consolidation
title: The multi-column sort comparator is hand-written six times, and People reimplements TableSortState
topic: viewer
status: done
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

## Done

Eleven loops, not six: the audit sampled `sl-viewer-people`, and the same loop
had since been written again in `settings_list.rs`, `experience_search.rs`,
`experiences_floater.rs`, `asset_blacklist.rs`, `avatar_render_floater.rs`,
`top_objects.rs` and `radar_model.rs`. `avatar_profile.rs:1398` was **not** one
of them any more — that list has a single-key sort with an id tie-break and no
table keys at all.

### The loop, once

`ui_table` gains three pure, generic items:

- `compare_by_sort_keys(keys, left, right, column_ordering, tie_break)` — the
  loop itself. Generic in how a column is *named*, because half the call sites
  name theirs by `&'static str` token and the radar by its own enum;
- `order_by_sort_keys(rows, …)` — the same over a whole list (a **stable**
  sort, so the tie-break is the last word and not the allocator's);
- `keep_order` — a named no-tie-break, so "this list falls back to its arrival
  order" reads as a decision rather than a `|_, _| Ordering::Equal` stub.

The contact-sets panel is why the *comparator* is public and not only the sort:
a set configured to sort by online status puts a key **ahead** of the table's,
so it wraps `compare_by_sort_keys` in its own `sort_by` rather than calling
`order_by_sort_keys`.

### `MultiSort<Column>`, and what is left of People's `SortState`

`TableSort` is now `MultiSort<usize>` — the click stack, `encode_with` /
`parse_with` and the comparator, generic in the column type, with the
index-keyed `tokens` / `encode` / `parse` in an `impl MultiSort<usize>` beside
it. `TableSortKey` is gone; a level is `(column, ascending)`, which is what the
consumers were converting it into anyway.

People's `SortState` is a newtype over `MultiSort<SortColumn>`: its `click`,
`compare`, `encode` and `parse` bodies (≈120 lines) are the widget's, and what
is left is the four things that are genuinely the friends list's — `SortColumn`,
`column_token` / `parse_column_token`, and `default_ascending`, the per-column
first direction (Online starts *descending*: a click on it is asking for the
online friends first). `MultiSort::click` takes that direction as an argument,
which is the one thing the widget's own always-ascending click could not
express.

### Why the friends list still has `builtin_sort: false`

The scope said "express People's eight columns as `TableColumn::token`s and
delete `SortState` in favour of `builtin_sort: true`". That half is **not**
done, deliberately: the eight columns a friends-list header click can name live
inside **four** table columns. The six permission columns are two
`TableColumnKind::Custom` cells, each a *two-row* grouped header — a
"They can …" / "You can …" label over three icon headers — and flattening them
to eight flat columns would drop those labels, which the widget cannot span. The
duplication the task was really about is gone either way; a widget that can
carry a spanning group header (and a per-column first sort direction in the
spec, which is a field on all 93 `TableColumn` literals) is its own task, and
one that trades a working grouped header for uniformity.

### The rest

- `TableState::sort_stamp()` returns `(revision, tokens)` in one read. The
  eleven view-rebuild systems each opened with the same twelve lines — read the
  table, `map_or(0, …)` the revision, then a `filter_map` over `SPEC.columns` to
  turn indices back into tokens — and now open with one;
- `apply_persisted_widths` / `encode_widths` are pinned by two tests: the
  round-trip, the clamp at both ends, and that a malformed field is dropped on
  its own rather than losing the good fields beside it. Writing them found the
  asymmetry: `encode_widths` writes **fixed** columns only, but
  `apply_persisted_widths` would take a width for a *flexible* one and overwrite
  the grow factor `TableState::widths` documents as unchanging — inert today
  (the live grow comes from the spec) and a silent relayout the day it does not.
  It now skips them, which is what the encoder always assumed;
- `radar_model::sort_rows` keeps its loop: it lives in `sl-viewer-kit`, which
  does not depend on `sl-viewer-ui-widgets` and should not start. It folds in
  when [[viewer-audit-kit-single-consumer-split]] moves `radar_model` to
  `sl-viewer-people`, where the helper is already in scope — `radar.rs` maps the
  table's tokens onto `SortColumn` today and would then stop needing to.
