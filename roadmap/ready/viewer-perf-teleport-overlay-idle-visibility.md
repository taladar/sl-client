---
id: viewer-perf-teleport-overlay-idle-visibility
title: Teleport progress overlay writes Hidden visibility every idle frame
topic: viewer
status: ready
origin: per-frame ECS churn audit (2026-08-09)
refs: [viewer-perf-minimap-compass-visibility-writes]
---

Context: [context/viewer.md](../context/viewer.md).

`render_overlay` (`sl-viewer-places/src/teleport_progress.rs`, Update,
no run condition) writes `*visibility = Visibility::Hidden` on `OverlayRoot`
**unconditionally every frame** in the idle case (`flow.entry == None`, the
overwhelmingly common state; still so at the 2026-10-15 audit). The overlay's
buttons/text are already equality-gated; the in-flight title colour write is
unconditional but transient.

Fix: `set_if_neq` the idle visibility write (and the title colour while at
it). Optionally add a run condition so the whole system skips when no
teleport is in flight and the overlay is already hidden — see
[[viewer-perf-run-condition-gating]] for the idiom.
