---
id: viewer-fps-label-intermittent
title: FPS status readout intermittently drops its "fps" label
topic: viewer
status: done
origin: user report during the world-frustum-culling profiling session
  (2026-08-01)
refs: [viewer-perf-world-frustum-culling-octree]
---

Context: [context/viewer.md](../context/viewer.md).

The on-screen FPS readout (the P19.1 diagnostics HUD / status-bar frame-rate
figure, `diagnostics.rs`) does not show the literal `fps` unit string all the
time — the number renders but the `fps` suffix is intermittently missing.

Likely a formatting / text-update path that rewrites the readout each frame and
occasionally emits only the number (e.g. a branch that writes the value without
the unit, a smoothed-value-not-yet-ready path, or width elision clipping the
suffix). Reproduce by watching the readout live; fix so the `fps` label is
always present alongside the value.

Low priority / cosmetic, but it is a persistent diagnostics-HUD glitch.

## Resolution

Not a formatting path — a layout / glyph-pass interaction. The read-out is the
status-bar FPS slot (`status_bar.rs`), not `diagnostics.rs` (which is now only
the pipeline-status overlay; its old frame-rate figure moved to the status bar).

Root cause: the fixed-width read-outs are marked `FixedSlotContentSize` so a
ticking value does not trip the layout gate (`ui_perf::ui_layout_dirty`) into a
full-tree relayout — the whole point of the fixed FPS slot. But `bevy_ui`'s
glyph pass (`text_system`, in the *ungated* `UiSystems::PostLayout`) reflows on
every text change, and it lays the glyphs inside the node's **last computed**
`content_box`, which is stale while layout is gated off. In the old config the
text node's width tracked its content, so when the proportional font made a new
value wider than the one layout last sized the node for (e.g. `88 fps` after
`11 fps`), `text_system` word-wrapped it into the stale-narrow box — dropping
`fps` onto a second line the slot's clip then hid.

Fix (`spawn_readout`): give the text a **content-independent** width so the
stale box is never too narrow, and pin the value to the correct edge.

- Trailing read-outs (balance / time / FPS) fill the slot (`flex_basis: 0` +
  `flex_grow: 1`) and right-justify their glyphs, so the unit sits at the
  trailing edge and any overflow clips the leading digits instead.
- Leading read-outs (region / coordinates / parcel name) keep their full text
  width (`flex_shrink: 0`) and never wrap (`TextLayout::no_wrap`), clipping the
  trailing tail against the leading-anchored slot.

The gate benefit is preserved: the FPS box ticking still does not relayout the
UI. Two client-side layout tests guard both branches
(`trailing_readout_width_does_not_track_its_value`,
`leading_readout_clips_rather_than_wrapping`).
