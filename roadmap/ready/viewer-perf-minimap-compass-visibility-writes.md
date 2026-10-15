---
id: viewer-perf-minimap-compass-visibility-writes
title: Minimap minor-compass labels write Visibility unconditionally
topic: viewer
status: ready
origin: per-frame ECS churn audit (2026-08-09)
refs: [viewer-perf-map-hover-tooltip-node-writes]
---

Context: [context/viewer.md](../context/viewer.md).

`layout_minimap_compass` (`sl-viewer-map/src/minimap.rs`) equality-gates its
`node.left`/`node.top` writes (`moved_visibly` — the good pattern) but
writes the four minor-diagonal label `Visibility` values unconditionally
every frame the panel is shown (still so at the 2026-10-15 audit).

`Visibility` is not a layout trigger, so this does not defeat the layout
gate; it does mark 4 entities `Changed<Visibility>` per frame, re-running
visibility propagation / UI extraction for them.

Fix: `vis.set_if_neq(...)` — one-line change per write.
