---
id: viewer-perf-ui-layout-per-frame-relayout
title: Cut per-frame UI layout cost — structural bevy_ui floor
topic: viewer
status: done
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

## Structural part — done 2026-08-01

Four levers, verified live on the local grid (the Aditi re-measure remains
owner-run acceptance):

1. **Name tags left `bevy_ui`** (`name_tag_overlay.rs`): each tag is now a
   `Text2d` on a dedicated overlay `Camera2d` (order 3, `IsDefaultUiCamera`
   so the UI's retarget to it is explicit; `Msaa`/`Hdr`/`Tonemapping` must
   match the world camera — mismatched HDR splits the view-target chain and
   blanks the window). `position_name_tags` writes a guarded, whole-pixel
   `Transform` — a moving tag no longer dirties taffy at all, and a
   stationary one writes nothing. Right-click-on-tag is a reusable
   screen-space rect test (`NameTagHitTest`, built for the future
   drag&drop-onto-avatar too); reflection probes structurally cannot see
   `Text2d`. The minimal stepping stone to
   [[viewer-name-tags-billboard-render]], which keeps distance culling.
2. **Lazy floater content** (`DeferredFloaterContent`, `floater.rs`): the
   startup node census (now permanent in `entity_diagnostics.rs`, tracy
   builds) showed **~2300 of 2625 UI nodes** sat in startup-spawned hidden
   floaters — hidden is `Display::None`, which taffy skips but every
   unconditional bevy_ui walk still visits. Eleven floaters ≥ ~40 nodes
   (build-tools 492, about-land 334, search 295, about-region 290,
   inventory 177, conversations 138, group-profile 104, inventory-filters
   72, worldmap 65, snapshot 60, avatar-profile 42) now spawn **chrome
   only**; content builds once on first open (a registered one-shot system
   run by `build_deferred_floater_content`) and is kept alive after.
   Openers resolve floaters **by stable id** (`floater_panel` /
   `toggle_floater`), never through the module's `XUi` resource — which
   only exists after the first open. Live census: **2625 → 687 nodes**.
   Not converted (small / observer-wired open paths): emoji-picker 58,
   color-picker 31, avatar-picker 30, minimap 28, web-browser 27,
   experiences 17.
3. **`ui_stack_system` gated** (`ui_perf::ui_stack_dirty`, a set-level
   `run_if` on `UiSystems::Stack` — sole member, no fork): rebuild only on
   node add/remove, UI hierarchy change, or z-index change, with world
   despawns filtered out of the removal messages. Measured during a
   10 s rez window: **1 rebuild in 604 frames** (was every frame, ~0.5 ms).
4. **`ui_layout_system` gated on visible changes**
   (`ui_perf::ui_layout_dirty` on `UiSystems::Layout`): the full trigger
   union of the system's inputs, minus changes buried under an unchanged
   `Display::None` strict ancestor — so a closed conversations floater
   receiving chat / presence updates defers its layout cost to the open
   (the `Display` flip always fires; the system's own change detection
   then sees everything it deferred). Removals always fire (the two-frame
   removal-message window). `SL_VIEWER_LOG_UI_DIRTY=1` logs what tripped
   the gate per frame plus a 5 s skip-rate line.

Deliberate no-gos, confirmed in-source: `update_clipping_system` shares
`UiSystems::PostLayout` with `text_system` (not set-gateable) **and** its
unconditional empty-clip is what keeps hidden subtrees invisible under the
layout gate; `ui_picking`'s `require_markers` would silently break every
`Button` without an explicit `Pickable` (neither `Node` nor the widgets
require one), and hidden nodes are already skipped by the zero-size
continue.

**Upstream find** ([[viewer-perf-editable-text-per-frame-churn]]): bevy_ui
0.19's editable-text systems mark every `EditableText` changed every frame
and re-`set` an *identical* `ContentSize` measure — with the always-visible
chat bar / menu search that alone re-laid-out the tree every frame (part of
the original floor). The gate carves editable `ContentSize` out and watches
the measure's real inputs (`TextFont`/`LineHeight`/`TextLayout`) instead.

Local-grid numbers (release + `profile-tracy`, 10 s windows; the "before"
row is the 2026-07-30 Aditi capture after the change-driven fixes):

| Measure | before | after |
| --- | --- | --- |
| UI nodes (`ENTITY_UI`) | 2625 | 687 |
| `ui_stack_system` | ~0.5 ms every frame | 1 run / 604 frames |
| `ui_layout_system` mean (rez) | 1.86 ms | 0.87 ms |
| `ui_layout_system` runs (steady) | every frame | 30/115, 33/186 frames |

The steady-state residue is the status bar's deliberate 10 Hz readouts (a
`ContentSize` change on one `status-readout` node per tick) — the gate runs
when the visible UI actually changed, which is the task's target: no more
unconditional full-tree layout on zero-change frames. During a rez burst
with real UI churn it correctly runs every frame.

Unit/headless tests: gate fire patterns (`ui_perf`), deferred-content
lifecycle (`floater`), tag projection + guard and the viewport→overlay
mapping (`name_tag_overlay`).

**Live acceptance (owner-run):** a `tracy-capture` pair (rez + idle) on
Aditi with the window focused — expect the layout/stack gates to skip on
quiet frames (watch `SL_VIEWER_LOG_UI_DIRTY=1`'s "ran X of Y frames"
line), tags tracking avatars with floaters above them, and every floater
opening with full content on first toggle.
