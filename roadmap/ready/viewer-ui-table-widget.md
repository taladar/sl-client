---
id: viewer-ui-table-widget
title: Reusable table widget (columns / header / virtualized rows / truncation)
topic: viewer
status: ready
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
