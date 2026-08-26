---
id: viewer-audit-ellipsis-reveal-latch
title: A cell about one ellipsis wide latches a permanent spurious ellipsis
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-widgets/src/ui_table.rs:1238` (and the copy at `ui_tab.rs:1228`) —
the ellipsis marker is `ChildOf(cell)`, a **sibling** of the clip container,
with `flex_shrink: 0.0`. Showing it shrinks the clip's `computed.size.x`, so
`content_size.x > size.x` stays true for any value whose natural width falls
between `cell_width - ellipsis_width` and `cell_width`. The state latches and
never clears.

The reveal system is copy-pasted: `ui_tab.rs:1223` and `ui_table.rs:1233` are
the same
`let truncated = computed.content_size.x > computed.size.x + f32::EPSILON;`
followed by the identical guarded `Display::Flex` / `Display::None` write,
differing only in the marker component (`TabLabelClip` vs `TableCellClip`); the
spawn bundles (`ui_tab.rs:1190-1208`, `ui_table.rs:1076-1093`) are verbatim
twins.

Proof the copy has already cost something: `ELLIPSIS_GAP` is `1.0` at
`ui_table.rs:69` and `2.0` at `ui_tab.rs:172`.

Scope: one generic `RevealEllipsis { marker: Entity }` component and one system,
measuring against the cell rather than the shrunk clip — which fixes the latch
once instead of twice.
