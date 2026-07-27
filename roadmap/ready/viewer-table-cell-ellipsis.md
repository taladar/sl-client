---
id: viewer-table-cell-ellipsis
title: Truncate overflowing table cells with a locale-aware ellipsis
topic: viewer
status: ready
origin: user request (2026-07-27), reviewing the group profile tables
refs: [viewer-social-group-profile]
---

Context: [context/viewer.md](../context/viewer.md).

The group profile's member / notice table cells now **clip** with `LineBreak::
NoWrap` + `overflow: clip` (they no longer wrap and misalign the rows), but a
too-long value is simply cut mid-glyph. The reference truncates with an
**ellipsis**, and the ellipsis glyph is **locale-specific** (Latin `…`, CJK
`……`) — the tab widget already does this (`crate::ui_tab` `DEFAULT_ELLIPSIS` +
its measure-and-truncate on a clipped strip).

Generalise that into a reusable table-cell truncation (measure the value against
the cell's laid-out width, truncate and append the locale ellipsis), and apply
it to the member columns (name / title / contribution / status) and the notice
columns (subject / from / date). Audit other fixed-width list cells (friends,
inventory) for the same treatment.
