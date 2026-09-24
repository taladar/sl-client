---
id: viewer-skin-tooltip-roles
title: The tooltip is a skinned surface everywhere but here
topic: viewer
status: done
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

## Done (2026-09-24)

**One tooltip, four consumers.** The viewer had four hand-spelled tips, not
the three this task named: the world hover tip, a link's URL tip, and the
**minimap's** and **world map's** hover cards, which each painted their own
`srgba(0, 0, 0, 0.8)`. All four now spawn through `skin::tooltip_box` /
`skin::tooltip_text` in `sl-viewer-ui-core`: an `.sk-tooltip` box and an
`.sk-tooltip-text` text node, with the fallback look beside each class for
the frame before the sheet lands. Their local colour constants are gone.

Eight tokens, colour *and* shape, because the reference varies both per
skin: `--tooltip-bg`, `--tooltip-border`, `--tooltip-text`,
`--tooltip-border-width`, `--tooltip-radius`, `--tooltip-padding-block`,
`--tooltip-padding-inline`, `--tooltip-max-width`. `skin.rs`'s `TOOLTIP_*`
constants repeat the fallback values, and
`tooltip_fallback_matches_the_fallback_sheet` holds the two together.

**The two non-negotiables are not style properties.** `tooltip_box` spawns
`Pickable::IGNORE` and `GlobalZIndex(skin::TOOLTIP_Z)` (`i32::MAX`) itself, so
no stylesheet can reach them and no consumer can forget them. The link tip had
been at `GlobalZIndex(1000)` — *below* the bottom bar (9000) — and the two map
tips had no z at all.

**The world hover tip was outside the styled tree.** It was spawned with no
parent, so a class on it would have resolved nothing (the same trap the
pipeline read-out hit in `viewer-skin-text-colours-without-a-role`). It is a
child of the UI root now, and two nodes instead of one `Text` with a
background, since box and text each need their own class.

The check is `a_skin_can_make_the_tooltip_a_light_plate`, against a
test-only `light-tooltip.css` that sets Vintage's tip (`#b7b8bc` plate, black
text, square, 4 px padded, 200 px wide) over the fallback's dark chrome. It
spawns through the real helpers and pins the three colours, the four lengths
(the padding is a two-`var()` shorthand, the one most likely to fail
silently), that the tip stays unpickable and topmost, and that the inspector
card does **not** take the tooltip's plate.

### A deviation: the inspector card has its own plate

The task listed "the inspector popups" among the tooltip consumers. The
reference disagrees: an inspector (`LLInspect`) is a floater with its own
nine-sliced `Inspector_Background`, it takes clicks where a tip must never
take one, and a skin that draws its tips as a light plate keeps its inspector
dark. So the card got a pair of its own — `--inspector-bg` /
`--inspector-border` on `.sk-inspector` — rather than the tooltip's. Its text
and buttons were already skinned through their own roles; the plate was the
last hardcoded part. Its z is now defined relative to `TOOLTIP_Z`.

### Deliberate visual changes

- The link tip wraps at 400 px (was 520) and takes the shared 5 × 8 padding
  (was 3 × 6).
- The plate is **opaque** in both shipped skins (`#0f121a` in Graphite). The
  first cut took the world hover tip's 0.75 scrim, and the live look caught it
  over a name link in chat; 0.97 (the link tip's old alpha) measured correctly
  but still read as a hole over the chat field, whose colour is close to the
  plate's. A tip is read over text, not only over the world.
- The world hover tip gains the 1 px frame the link tip had.
- The map tips gain the frame, the rounded corners, the wrap width and the
  wider padding (were 4 px all round, unframed, unbounded).

Font sizes stay per consumer (14 px world tip, 12 px link and minimap, the
map's panel size) — that is typography, not the plate.

### Found on the way: an untranslated theme

The live look also turned up `relief` showing its raw Fluent key in the
preferences theme combo: the combo builds `preferences-theme-<id>` from the id,
and the theme shipped without the label. It has one now, and
`every_shipped_skin_and_theme_has_a_label` (in `shipped_skins.rs`, through the
tab's own `skin_label_key` / `theme_label_key`, made `pub` for it) fails the
next skin or theme that ships without one.
