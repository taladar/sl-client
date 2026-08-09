---
id: viewer-perf-worldmap-label-node-writes
title: World-map region labels write Node unconditionally every frame
topic: viewer
status: ready
origin: per-frame ECS churn audit (2026-08-09)
refs: [viewer-perf-ui-layout-gate-open-widget-churn, viewer-perf-frame-churn-cleanups]
---

Context: [context/viewer.md](../context/viewer.md).

`layout_world_map_labels` (`world_map.rs:1815-1922`, runs every frame while
the world-map floater is open with names on) writes `node.left` / `node.top`
for **every visible region label** with no equality guard
(`world_map.rs:1901-1904`) and `*visibility = Visibility::Inherited`
unconditionally (`world_map.rs:1911-1913`, plus `Hidden` on the unused tail).

Any `Changed<Node>` on a visible UI entity trips `ui_perf::ui_layout_dirty`,
so this defeats the layout gate outright: **full taffy relayout + parley text
reshape every frame the world map is open**, even on a completely static map.
This is the loudest UI churn item of the audit.

Fix: add the same `!=` guard on `left`/`top` that `layout_minimap_compass`
already uses (`minimap.rs:2247`, with a comment naming the layout gate), and
`set_if_neq` the visibility writes. Bounded by `MAX_LABELS` entities.

Deliberately **not** done on the `performance` branch (2026-08-09): a parallel
agent owns the UI code and edits there would conflict.
