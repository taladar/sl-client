---
id: viewer-perf-minimap-compass-visibility-writes
title: Minimap minor-compass labels write Visibility unconditionally
topic: viewer
status: ready
origin: per-frame ECS churn audit (2026-08-09)
refs: [viewer-perf-map-hover-tooltip-node-writes]
---

Context: [context/viewer.md](../context/viewer.md).

`minimap::layout_minimap_compass` (`minimap.rs:2188-2253`) equality-gates its
`node.left`/`node.top` writes (`minimap.rs:2247` — the good pattern) but
writes the four minor-diagonal label `Visibility` values unconditionally
every frame the panel is shown (`minimap.rs:2220-2226`).

`Visibility` is not a layout trigger, so this does not defeat the layout
gate; it does mark 4 entities `Changed<Visibility>` per frame, re-running
visibility propagation / UI extraction for them.

Fix: `vis.set_if_neq(...)` — one-line change per write.

Deliberately **not** done on the `performance` branch (2026-08-09): a parallel
agent owns the UI code and edits there would conflict.
