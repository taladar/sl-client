---
id: viewer-perf-map-hover-tooltip-node-writes
title: Minimap / world-map hover tooltips write Node + Visibility unconditionally
topic: viewer
status: ready
origin: per-frame ECS churn audit (2026-08-09)
refs: [viewer-perf-ui-layout-gate-open-widget-churn, viewer-perf-worldmap-label-node-writes]
---

Context: [context/viewer.md](../context/viewer.md).

Two systems share the same ungated pattern:

- `update_minimap_hover` (`sl-viewer-map/src/minimap.rs`): while the cursor
  is over the minimap surface it writes the tooltip's `node.left`/`node.top`
  with no equality guard, and writes the tooltip `Visibility`
  unconditionally every frame **regardless of hover**.
- `update_world_map_hover` (`sl-viewer-map/src/world_map.rs`): identical —
  ungated tooltip `node.left/top` while hovering and unconditional tooltip
  visibility.

(Audit 2026-10-15: both still write unguarded; the modules moved out of the
viewer crate into `sl-viewer-map`, so the old line numbers are gone.)

The `Node` writes trip `ui_perf::ui_layout_dirty` → full relayout + text
reshape **every frame while the cursor rests on a map surface** (a common
resting place). The tooltip *text* in both systems is already equality-gated
— only the node/visibility writes regressed.

Fix: `!=`-guard the `left`/`top` writes (skip when the cursor did not move a
whole pixel) and `set_if_neq` the visibility writes.
