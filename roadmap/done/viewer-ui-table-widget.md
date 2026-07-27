---
id: viewer-ui-table-widget
title: Reusable table widget (columns / header / virtualized rows / truncation)
topic: viewer
status: done
refs:
  [viewer-ui-virtualized-list, viewer-social-group-profile, viewer-social-groups,
  viewer-social-people-panel, viewer-table-cell-ellipsis]
---

Context: [context/viewer.md](../context/viewer.md).

Several surfaces have each hand-rolled the *same* table: a fixed header over
rows with a mix of flexible and fixed-width columns — the group profile's
members / notices tables, the People friends / groups lists, inventory columns,
etc. Each re-solves (often inconsistently) column widths, header↔row alignment,
per-cell clipping / no-wrap, ellipsis truncation, and row virtualization. The
result is the bugs seen while reviewing the group profile: columns that wrap and
misalign, cells cut mid-glyph instead of an ellipsis, headers drifting from
their rows.

Build **one** reusable table widget on top of [[viewer-ui-virtualized-list]]:

- a column spec (fluent-key header, width = fixed px or flex-grow, alignment)
  and a header row derived from it, guaranteed aligned with the body cells;
- cells that clip + no-wrap and truncate with a **locale-aware ellipsis** (fold
  in [[viewer-table-cell-ellipsis]] / reuse the tab widget's
  measure-and-truncate);
- virtualized, recycled rows bound from a caller-supplied projection (the
  current `populate_*`/`bind_*` pattern), with per-row selection + a row
  press/double-press observer hook;
- optional column sorting (the friends list already sorts).

Then migrate the existing tables (group profile members/notices, People
friends/groups, inventory) onto it, deleting their bespoke header/cell/clip
code. This subsumes [[viewer-table-cell-ellipsis]] as the truncation half.

## Done (2026-07-27)

Built `sl-client-bevy-viewer/src/ui_table.rs` — a reusable table on top of
[[viewer-ui-virtualized-list]]:

- **Column spec** (`TableColumn`): fluent header key + stable persistence
  token, `TableColumnWidth::{Flex, Fixed}`, `TableAlign::{Start, Center, End}`,
  a `sortable` flag, and a `TableColumnKind::{Text, Custom}` — `Text` cells are
  widget-owned (clip + no-wrap + locale ellipsis), `Custom` cells are an empty
  sized container the consumer fills (icons / grouped sub-headers).
- **Header** derived from the columns, aligned with the body because both carry
  `TableColumnCell` and one `sync_table_column_widths` system writes each
  column's width. **Draggable column widths** (fixed columns) and **click
  sort** (multi-level, `▲`/`▼` on the primary), both **persisted per avatar**
  via `ViewerSettings` (`sort`/`widths` settings per table). A `builtin_sort`
  opt-out lets a consumer keep its own ordering.
- **Locale-aware ellipsis**: folded the tab widget's and the table's markers
  onto one `i18n::LocaleEllipsisMarker` (subsumes [[viewer-table-cell-ellipsis]]
  — that item is done too).

Migrated onto it: **group profile members / notices / roles** (bespoke
header/cell/clip deleted; roles went from a single-column retained list to a
Name/Title/Members sortable table; notices gained a real header) and the
**People friends list** (Name/Status as widget text columns — Name gains the
ellipsis — with the permission rights-grid kept as widget *custom* columns so
its grouped header + bespoke 8-way sort survive; the widget owns header
alignment, resizable+persisted widths, scroll, row pool and selection). The
People **Groups** tab is empty (groups live in the Groups floater), so nothing
to migrate there.

**Deferred** (agreed with the user): the **inventory** columns (a folder
*tree*, not a flat table — a larger redesign) and a **full** People rights-grid
rebuild inside the widget. Tracked in [[viewer-table-widget-remaining]].
