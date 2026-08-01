---
id: viewer-perf-ui-layout-gate-open-widget-churn
title: Layout gate defeated per-frame by the minimap compass and status readouts
topic: viewer
status: done
origin: Tracy profiling of Aditi rezzing (2026-08-01) — full 2-min capture
refs:
  [
    viewer-perf-ui-layout-per-frame-relayout,
    viewer-perf-editable-text-per-frame-churn,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

A full ~2-minute Tracy capture of rezzing on Aditi (548 MB, 26 M zones) showed
`bevy_ui::layout::ui_layout_system` running on **4960 of 5053 frames (98 %)** at
~1.18 ms/frame — i.e. the [[viewer-perf-ui-layout-per-frame-relayout]] gate was
**not skipping**, despite that task reporting the gate closed on quiet frames.
That task's live acceptance was on the local grid; two per-frame writers only
churn under conditions it did not exercise, so they slipped through.

Root cause, from the `SL_VIEWER_LOG_UI_DIRTY=1` gate meter on Aditi (gate ran
**99 %**, 2820/2847 frames):

- **`node` trigger — the minimap compass.** `layout_minimap_compass`
  (`minimap.rs`) repositions the 8 compass labels **every frame** and wrote
  `node.left`/`node.top` **unconditionally**, so `Changed<Node>` fired on ~8
  compass nodes per frame (`minimap-compass ×8396` over the window) whenever the
  minimap is open — which it is on Aditi but was not on the local-grid
  acceptance. This was the dominant driver.
- **`content-size` trigger — the status readouts.** The FPS integer re-measures
  at the readouts' 10 Hz refresh (`status-readout` content-size), tripping the
  gate ~10×/s. Minor on its own (~19 % at 35 fps) but real.

## Fix

1. **Compass write-on-change** (`layout_minimap_compass`): compute the
   label offsets, then assign `node.left`/`node.top` only when they differ.
   A still camera stops dirtying `Node` entirely; a turning compass still
   relayouts (correct — the labels are absolutely positioned and need their
   own layout to move, so the gate must *not* absorb them or the compass
   would freeze). Mirrors the `update_parcel_icons` write-on-change convention.
2. **`FixedSlotContentSize` marker** (`ui_perf.rs`): an opt-in component telling
   `ui_layout_dirty` to ignore a node's `Changed<ContentSize>` — the same
   carve-out shape as the `EditableText` one. The caller asserts the invariant:
   the node lives in a fixed-width (`Val::Px`, `flex_shrink: 0`) clipping slot
   and is single-line, so its measure can neither resize that slot (clip
   suppresses the min-content minimum) nor escape it. An automatic
   "fixed-size ancestor" walk cannot do this here — the menu-bar row and status
   slots have content-derived (`Auto`) heights all the way up, so no ancestor is
   definite on both axes; only the caller can vouch for the width-plus-clip
   guarantee. `status_bar.rs` marks the fixed-width readouts (region, coords,
   balance, time, FPS); the flexible parcel name is left unmarked.

Residual: a trailing-aligned readout can render ~1 digit-width off for a frame
when its digit count changes during pure idle (nothing else dirtying layout) —
clipped in its fixed slot, refreshed by any interaction. Effectively never in
practice: the FPS is capped at 60 so it stays two digits (a sub-10 fps stall
would be needed), and balance/time rarely change width. Accepted, per the
"a fixed-size slot should not force a relayout" intent.

## Verified live (Aditi, minimap + inventory open, `SL_VIEWER_LOG_UI_DIRTY=1`)

| Measure | before | after |
| --- | --- | --- |
| layout gate run rate | 99 % (2820/2847) | **1 % (35/6983)** |
| `minimap-compass` `Changed<Node>` | ×8396 | ×84 (only while turning) |
| status-readout content-size | fires gate | carved out (ignored) |

The gate now sits at **0 %** on quiet frames and rises only during genuine churn
(camera rotation, menu use). Unit test:
`ui_perf::tests::layout_gate_ignores_fixed_slot_content_size`.

The same capture's frame-time distribution (for the record, shutdown tail
excluded): mean 27.9 ms / p50 25.7 / p95 47.3 / p99 78.3, ~35.8 fps. Sustained
cost beyond this gate is dominated by visibility/frustum culling (~17 ms/frame
of parallel work, scales with region entity count) — a separate, untouched
lever tracked under the other `viewer-perf-*` items.
