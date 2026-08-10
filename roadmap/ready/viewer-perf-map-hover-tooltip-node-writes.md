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

- `minimap::update_minimap_hover` (`minimap.rs:2288-2394`): while the cursor
  is over the minimap surface it writes the tooltip's `node.left`/`node.top`
  with no equality guard (`minimap.rs:2375-2378`), and writes the tooltip
  `Visibility` unconditionally every frame **regardless of hover**
  (`minimap.rs:2381-2387`).
- `world_map::update_world_map_hover` (`world_map.rs:1938-1989`): identical —
  ungated tooltip `node.left/top` while hovering (`world_map.rs:1971-1974`)
  and unconditional tooltip visibility (`world_map.rs:1977`).

The `Node` writes trip `ui_perf::ui_layout_dirty` → full relayout + text
reshape **every frame while the cursor rests on a map surface** (a common
resting place). The tooltip *text* in both systems is already equality-gated
— only the node/visibility writes regressed.

Fix: `!=`-guard the `left`/`top` writes (skip when the cursor did not move a
whole pixel) and `set_if_neq` the visibility writes.

Deliberately **not** done on the `performance` branch (2026-08-09): a parallel
agent owns the UI code and edits there would conflict.
