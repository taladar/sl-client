---
id: viewer-text-node-padding-measure
title: Upstream bug — padding/border on a bevy_ui Text node resolves the wrap width wrongly
topic: viewer
status: bugs
origin: found by hand-testing the F5 scaffold demo panel (2026-07)
refs: [viewer-ui-test-harness, viewer-ui-widget-scaffold, viewer-ui-text-foundation]
---

Context: [context/viewer.md](../context/viewer.md).

A `bevy_ui` **`Text` node that carries its own `padding` or `border`** is laid
out with the wrong wrap width, so wrapping text gets
**one fewer line than it draws** and the node ends up one line shorter than its
own content. The last line then hangs out of the bottom of whatever contains it.

Found in the `F5` demo panel of [[viewer-ui-widget-scaffold]]: the sample label
(a `Text` with `padding` left 24 / right 8 and a 4 px leading `border`) rendered
its last line's descenders below the panel's background.

## Measured

Live, at window scale factor 1.5, values physical:

| node | `size` | `content_size` |
| --- | --- | --- |
| `Text` with padding + border | `(669, 94)` | `(633, 121)` |
| the same text wrapped in a decorated container | `(750, 82)` | `(720, 82)` |

The 27 px shortfall is exactly one line (18 logical px) at that font size. The
telling number is the width: the text laid out at **422 logical** while its
content box was only **410** — 12 px too wide, which is precisely the *trailing
padding (8) plus the border (4)*. So the measure appears to subtract the leading
padding but not the trailing padding or the border, over-estimates the available
width by that much, fits one more word per line, and arrives at one fewer line.

Not flex shrink: `flex_shrink: 0` on the containing panel changed nothing, and
the root had 2160 px of space for 483 px of content.

## Worked around, not fixed

`crate::ui`'s demo puts the decoration on a **container** and leaves the `Text`
a plain child, which makes `size` and `content_size` agree for *that* panel.
That is the better structure regardless — a text run is not a box — so the
workaround is not a wart, and the convention is worth keeping even after this is
fixed.

## The harness landed, and the bug is bigger than this

[[viewer-ui-test-harness]] is done, and this was its first customer. Two things
changed.

**There is now a headless repro.** `sl-client-bevy-viewer/src/ui_test.rs`,
`tests::a_text_node_may_not_carry_its_own_padding` — no grid, no window, no
login, ~0.2 s. It asserts the bug is **still present**, so it is also the
canary: it starts failing the day Bevy fixes the measure, which is when the
workarounds below can go. (The premise this file recorded — that `bevy_ui`'s
headless layout is `pub(crate)` and would need upstreaming — was
**wrong for 0.19**: `ui_layout_system`, `propagate_ui_target_cameras`,
`UiSurface` and `measure_text_system` are all `pub`. No fork was needed.)

**The matrix found it is not only about padding on a `Text` node.** The measure
loses **anything that narrows a text node's available width other than its own
parent's padding**, and it does so silently. Measured headlessly, 15 px text in
a 420 px container:

| what narrows the text | it measured | its box was | over by |
| --- | --- | --- | --- |
| a 4 px border on the container | 388 | 384 | 4 |
| a 4 px sibling node beside it | 390 | 387 | 3 |
| nothing (padding only) | 390 | 388 | 2 |

So the workaround recorded above — decorate a container, leave the `Text` a
plain child — **is not sufficient**: a container with a border, or with anything
sharing its inline axis, still leaks that width into the child's wrap. This is
why `ui_element::spawn_label` carries no decoration beside its text at all.

None of it is visible in English, where the wrap lands short of the boundary by
luck. It shows up in Arabic and under pseudolocalisation, which land on it.

Two further properties, both measured, both worth putting in the upstream
report:

- **It does not accumulate with nesting.** A three-deep tree reports the *same*
  overshoot at every level — text 551/546, its box 599/594, the panel 635/630,
  all 5 px — not 5/10/15. One error at the text measure, propagated outward
  unchanged by each ancestor's `content_size`. So it is **not** per-level pixel
  rounding, which was the natural first guess.
- **It scales with the font, not the display.** ≈ **0.23 × the font size** — 5
  logical px at 22 px text, 3.5 at 15 px — and near-constant against both
  `scale_factor` and `UiScale` once converted to logical. Roughly a quarter em,
  which smells like a per-line advance the measure does not account for.

That last one is why `ui_test::OVERFLOW_EPSILON` is 6 logical px rather than 1:
until this is fixed, no layout check in this workspace can resolve finer than a
quarter em. Fixing this upstream tightens every UI check we have.

## To do

Report (or fix) it upstream, now that there is a repro worth attaching. Per
`sl-client-fork-upstream-for-upstream-bugs` the shape is: minimal repro → issue
→ fix → `[patch.crates-io]` until it lands.

Suspect (unchanged): `bevy_ui-0.19.0/src/widget/text.rs` —
`TextMeasure::measure`, and how `MeasureArgs::available_width` relates to the
node's content box. The new evidence says the question is broader than padding:
what does `available_width` get for a node whose width is constrained by its
*parent's* border, or by a sibling?
