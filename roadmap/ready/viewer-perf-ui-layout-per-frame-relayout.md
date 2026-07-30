---
id: viewer-perf-ui-layout-per-frame-relayout
title: Cut per-frame UI layout cost — structural bevy_ui floor
topic: viewer
status: ready
origin: Tracy profiling of Aditi rezzing (2026-07-30)
refs: [viewer-perf-minimap-layer-raster-offthread]
---

Context: [context/viewer.md](../context/viewer.md).

Tracy self-time over the first ~10 s of rezzing on Aditi shows the UI layout
stack as the largest system-time category (~4.4 ms/frame at ~20 fps), even
though the world is what is rezzing:

| System | ms/frame (before) |
| --- | --- |
| `bevy_ui::layout::ui_layout_system` (taffy) | 2.43 |
| `bevy_ui::stack::ui_stack_system` | 0.59 |
| `bevy_ui::update::update_clipping_system` | 0.45 |
| `bevy_ui::widget::text::text_system` | 0.32 |
| `bevy_ui::picking_backend::ui_picking` | 0.31 |
| `bevy_flair_style::…::calculate_styles_and_set_vars` | 0.32 |

## Change-driven part — fixed 2026-07-30 (committed)

Two systems dirtied UI `Text` on almost every frame; both fixed:

- **i18n demo panel** (`i18n::update_i18n_demo_text`) recomputed ~14
  translated/formatted lines every frame even while the F6 panel was hidden.
  Gated on visibility with a `run_if`. Measured: the system went from n=203
  (every frame, ~0.095 ms/f) to **0 invocations** while hidden.
- **status bar** (`status_bar::update_status_readouts`) rewrote the FPS readout
  every frame (the FPS integer keeps changing), forcing per-frame text re-shape
  / re-measure. Throttled to 10 Hz (`run_if(on_timer(100 ms))`) — status text
  needs no more. Measured: n dropped to ~10 Hz; `text_system` **max 4381 → 541
  µs, mean 322 → 74 µs**; `measure_text_system` mean **185 → 81 µs**.

## The remaining cost is structural, not change-driven

A/B re-profile after the two fixes: `ui_layout_system` **mean fell 2469 → 1863
µs but its *min* was unchanged, 1422 → 1401 µs**. With all per-frame UI text
churn removed (i18n gone, status at 10 Hz → many frames now have zero UI
change), the layout system still costs ~1.4 ms **every** frame. `ui_stack`
(~0.5 ms) and `update_clipping` (~0.15 ms) have the same non-zero floors. So the
dominant cost is bevy_ui's **unconditional per-frame** work — O(node count)
traversal / taffy bookkeeping — independent of what changed. Note the status bar
is already well-architected (each readout is a fixed-width slot with the text in
a child node, `status_bar.rs`), so a value change never propagates layout to
siblings; the floor is not our text nodes forcing a whole-tree relayout.

Remaining work (this is the real task now):

- **Reduce the live UI node count.** Count nodes in the always-present tree
  (top bar, minimap, chat, any hidden-but-`Display`-laid-out panels) and cut /
  collapse where possible; a hidden floater that is still `Display`-laid-out
  still costs every frame.
- **Investigate a bevy_ui early-out**: whether `ui_layout_system` /
  `ui_stack_system` can skip when no `Node` / `ContentSize` / children changed
  this frame (taffy already caches; the per-frame *iteration* is the floor).
  If it belongs upstream, follow the fork-upstream policy
  (`sl-client-fork-upstream-for-upstream-bugs`).
- Confirm `ui_picking` / `update_clipping` need to run every frame.

Re-measure with `tracy-grab.sh` (a ~10 s window right after the handshake is the
heaviest rezzing slice; a longer one is fine too — the earlier "30 s exceeds the
Tracy loader" claim was a truncated-file / Tracy-loader bug, not a size limit,
see the profiling doc): the target is `ui_layout_system` **min** dropping on
zero-change frames.
