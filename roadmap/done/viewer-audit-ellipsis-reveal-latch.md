---
id: viewer-audit-ellipsis-reveal-latch
title: A cell about one ellipsis wide latches a permanent spurious ellipsis
topic: viewer
status: done
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

## Outcome (2026-09-04): one module, three callers, and a third copy

There were **three** copies, not two: `sl-viewer-inventory`'s row labels carried
the same pair, with the overflow test split out as a `pub fn
ellipsis_visible(&ComputedNode)` and the same `ELLIPSIS_GAP`, so the finding's
"copy-pasted" reading was right and understated.

All three now go through `sl-viewer-ui-core/src/ui_ellipsis.rs`:
`RevealEllipsis { marker }` on the node whose overflow decides,
`spawn_ellipsis_marker` for the marker itself, the pure `ellipsis_wanted`, and
one `apply_reveal_ellipsis` registered by `ViewerUiPlugin` (and by the layout
harness, so a fixture that clips behaves as it does live). `TabLabelClip`,
`TableCellClip`, `ellipsis_visible`, the two divergent `ELLIPSIS_GAP` constants
and the three systems are gone.

The measure is **not** "against the cell", as the finding scoped it: the
inventory's marker is one of six children of its row, so the cell's width is not
the value's budget. It is against *the width the value would have with the
marker hidden* — the clip's laid-out width plus whatever the marker occupies
right now — which is the same number in both states, so the predicate has no
memory of its own answer. That is also why the marker's leading gap moved from a
physical `margin: left` to a `LogicalPadding`: padding is inside the border box
layout reports as `size`, so "what the marker occupies" is one physical number
rather than a size plus a logical margin to scale and add. Mirroring under RTL,
which the physical margin never did, comes along for free.

`f32::EPSILON` was a no-op at these magnitudes (`100.0 + 1.19e-7 == 100.0`); the
tolerance is now half a physical pixel, named and explained.

## The reveal was inert on the tab side, and the regression test proved it

`a_widened_strip_takes_the_ellipsis_off_a_label_that_fits_again` is a
real-layout test that calibrates the latching band off its own measurements — it
widens the strip until the label has its natural width plus *half* a marker,
which no font metric can move out of the band — and it could not reach the state
it was written to test: the marker never appeared at all.

`TabPlacement::viewport_node` set `min_width: Px(0)` on the **horizontal**
viewport only. A flex item's `min-width: auto` resolves to its min-content
width, and a no-wrap label has no break opportunity, so a vertical strip's
viewport could not shrink below the longest tab name: pinned to 70 px, the
strip's buttons laid out at 204 px and spilled out of it, and nothing ever
overflowed the label clip. So the tab widget's ellipsis had never been revealed
by anything.

Fixed with the missing `min_width`, which is what makes the label clip and the
marker show. The other two callers were unaffected — their containers were
already shrinkable.
