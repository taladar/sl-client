---
id: viewer-perf-ui-static-relayout
title: UI layout/text/styling runs every frame on a static UI
topic: viewer
status: ideas
origin: Tracy capture during the tessellation-cache verification (2026-07-31)
refs: [viewer-profiling, viewer-perf-frame-churn-cleanups]
---

Context: [context/viewer.md](../context/viewer.md).

A 1266-frame Tracy capture (login + interactive camera, inventory floater
open) shows the UI stack burning a steady ~6 ms of **self time per frame**
even while the UI is visually static:

- `bevy_ui::layout::ui_layout_system` — 3.80 s total ≈ **3.0 ms/frame**
- `bevy_ui::widget::text::text_system` — 1.55 s ≈ **1.2 ms/frame**
- `bevy_ui::widget::text::measure_text_system` — 0.70 s ≈ **0.55 ms/frame**
- `bevy_flair_style` systems (calculate_styles, tick_animations,
  calculate_effective_style_sheet, resolve_property_values ×2) — together
  ≈ **1.4 ms/frame**, plus two `par_for_each` flair queries at ~160 calls
  per frame each.

That is over a third of a 60 Hz frame budget spent re-laying-out, re-shaping
and re-styling a UI that did not change. Suspected drivers, to verify:

- Per-frame text writers (the status-bar FPS/clock readouts — already a
  [[viewer-perf-frame-churn-cleanups]] item — and any other every-frame
  `Text` rewrite) dirtying taffy/text so the full layout pass never goes
  quiet.
- `tick_animations` and the resolve/calculate flair passes running
  unconditionally rather than only while an animation or style change is
  live (composes with [[viewer-perf-run-condition-gating]]).
- `measure_text_system` re-measuring text whose content did not change.

Fix direction: make the per-frame writers change-gated (write only when the
rendered string actually changed, throttle the readouts), then check
whether `ui_layout_system` and the flair passes go quiet on an idle frame;
gate what remains behind change detection / run conditions where the
upstream crates allow. Measure the same three zones before/after on an
idle scene ([[viewer-profiling]]).
