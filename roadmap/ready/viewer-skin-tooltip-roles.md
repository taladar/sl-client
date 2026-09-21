---
id: viewer-skin-tooltip-roles
title: The tooltip is a skinned surface everywhere but here
topic: viewer
status: ready
origin: Vintage skin fidelity audit (2026-09-20)
points: 2
refs: [viewer-hover-tooltips, viewer-ui-skin-tokens, viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

`hover_tooltip.rs` `setup_hover_tooltip` spawns the tooltip with its look
written inline: `srgba(0, 0, 0, 0.75)` behind `srgb(0.95, 0.95, 0.95)` text,
8×5 padding, a 4 px radius. Nothing about it can be skinned, and the same
values are re-spelled by the other things that pop a tip (the link tooltip in
`linkified_text.rs`, the inspector popups).

The reference treats the tooltip as a first-class skinned widget —
`tool_tip.xml` per skin — and Vintage's is the opposite of ours in every
respect: a **light** `#b7b8bc` nine-sliced plate with a darker top edge,
**black** text (`ToolTipTextColor`), `max_width="200"`, `padding="4"`, square
corners. On a skin whose floaters are dark grey, a light tip is what separates
"this is the viewer telling you something" from "this is part of the panel".

## What to do

- `--tooltip-bg`, `--tooltip-text`, `--tooltip-border` roles and a
  `.sk-tooltip` / `.sk-tooltip-text` pair in `common.css` (two rules, because
  `bevy_ui` has no style inheritance).
- The world hover tooltip, the link tooltip and the inspector popups all adopt
  them; their local colour constants go.
- Padding and max width come from the same rule, so a skin can make a tip
  narrower or tighter — the reference varies both per skin.
- The tip must keep the two properties it has that are not negotiable:
  `Pickable::IGNORE` and `GlobalZIndex(i32::MAX)`. A skin may restyle a
  tooltip; it may not make one swallow a click or fall behind a floater.

## Done when

Tooltip colours, padding and width come from the skin, all three tip
consumers share the roles, and a scratch skin can make the tip a light plate
with black text without touching Rust.
