---
id: viewer-text-node-padding-measure
title: Upstream bug — a text node was measured at its border box, and a hanging space counted as content
topic: viewer
status: done
origin: found by hand-testing the F5 scaffold demo panel (2026-07)
refs: [viewer-ui-test-harness, viewer-ui-widget-scaffold, viewer-ui-text-foundation,
  viewer-grid-row-height-from-unwrapped-text]
---

Context: [context/viewer.md](../context/viewer.md).

A `bevy_ui` **`Text` node was laid out with the wrong wrap width**, so wrapping
text got fewer lines than it drew and the last ones hung out of the bottom of
whatever contained it. Found in the `F5` demo panel of
[[viewer-ui-widget-scaffold]]: the sample label (a `Text` with `padding` left 24
/ right 8 and a 4 px leading `border`) rendered its last line's descenders below
the panel's background.

Once [[viewer-ui-test-harness]] could measure it headlessly, the one description
turned out to cover **three separate defects**. Two are fixed here; the third is
filed as [[viewer-grid-row-height-from-unwrapped-text]].

## 1. The wrap width came from the border box — fixed

`TextMeasure::measure` (`bevy_ui/src/widget/text.rs`) preferred taffy's
`known_width` over `available_width`:

```text
let x = width.effective.unwrap_or_else(|| match available_width { … })
```

`known_width` is the node's **border box**. `available_width` is the same width
with the node's own padding, border and scrollbar gutter already subtracted —
taffy does that in `compute_leaf_layout` before it ever calls a measure. So a
text node carrying its own padding was wrapped at a width the renderer never
wraps at (`text_system` lays out at `node.content_box()`), and its height came
back for that wider wrap.

Measured, 15 px text, a 600 px column, 150 px of padding a side on the text node
— a 300 px content box:

| | node height | what it is |
| --- | --- | --- |
| the padded node | 54 | 3 lines — the height of a **600 px** wrap |
| the same text in a 300 px column | 90 | 5 lines — the wrap it actually gets |
| the same text in a 600 px column | 54 | the wrap it was measured at |

Two lines of text drawn outside the box. The original F5 panel had only 12 px of
trailing padding + border, which is one line's worth at that width — hence "one
fewer line", and hence a decade of this looking like a rounding problem.

The fix takes `available_width` and drops `width.effective`. It cannot be done
by subtracting the padding inside the measure: taffy's style padding is a
`LengthPercentage`, and a percentage resolves against a parent width the measure
is not given.

## 2. A hanging space was counted as content — fixed

`buffer_dimensions` (`bevy_text/src/pipeline.rs`) sized the text from
`parley::Layout::full_width`, which **includes trailing whitespace**. Parley
hangs the space a soft wrap breaks at past the wrap width — `line_break.rs`:
*"We hang any overflowing whitespace and then line-break"* — exactly as CSS
does. So the measured content width came back up to one space advance (≈ a
quarter em) **wider than the bounds it was just given**, and every ancestor's
`content_size` inherited it.

This is the overshoot the matrix characterised, and its properties fall out of
that mechanism: it does not accumulate with nesting (one error, propagated
outward unchanged), and it scales with the font rather than the display (a space
is a fixed fraction of an em). It is data-dependent, because the excess is
whatever is left of the space after the last word on the widest line — at 15 px
text in a 420 px column it showed as 390 against a 388 px box, while the same
text in a 424 px column was clean.

**`width` is not the fix, and finding that out is the interesting part.** Sizing
from `Layout::width` (whitespace excluded) shrinks an *unbounded* run to its
ink, and a run ending in a space, laid out again at exactly that width, breaks
at the space it can no longer hold and takes a second line it draws nothing on.
The `linkified-text` element is made of such runs — every plain segment of it
between two links ends in one — and it went two lines tall in **every cell**,
plain English at scale 1 included:

```text
"See "  size 28x18  content 28x36     <- width alone
"See "  size 32x18  content 32x18     <- as shipped
```

So the rule is: count the hanging space where it **fits inside the width the
layout was broken at** (parley records that as `layout_max_advance`), and drop
it where it does not. The half of the pair that catches the simpler answer is
`measure_tests::a_shrink_to_fit_run_ending_in_a_space_stays_one_line`.

## 3. A grid row sized from a wrap that never happens — split out

Split out to [[viewer-grid-row-height-from-unwrapped-text]], and since fixed
there. It turned out not to be grid track sizing at all but two `taffy` flexbox
and cache bugs; the description below is the one it was filed under: an auto
grid row takes its height from a layout at the item's max-content width and
never re-measures. Every root node is a grid item (`bevy_ui` parents each to an
implicit viewport node that is a grid), so the original headless canary — a
bounded panel spawned as its own root — was firing on *this*, not on the
padding. That is why the canary went on passing while the padding half was, on
the evidence, already harmless in every fixture laid out under `UiRoot`.

## What landed

- The two fixes, in the Bevy fork (`sl-client-bevy-pbr-fork`), on the pinned
  rev.
- `sl-viewer-testkit`'s `measure_tests`: `a_text_node_wraps_at_its_content_box`
  (three layouts, no pixel constants),
  `a_wrapped_text_node_is_never_wider_than_its_box` (a width sweep, because a
  single width misses it) and
  `a_shrink_to_fit_run_ending_in_a_space_stays_one_line`, plus the inverted
  canary for the grid bug.
- The old canary `a_text_node_may_not_carry_its_own_padding` is **gone**. It was
  firing on defect 3, not on padding: its fixture was a bare root, which is a
  grid item of the implicit viewport node. Its partner
  `the_same_text_in_a_decorated_container_is_clean` stays, as the check that the
  convention lays out cleanly.
- The convention (decoration on a container, the `Text` a plain child)
  **stays**. It is the right structure regardless — a text run is not a box —
  and it is now documented as a convention rather than as a workaround.

## What the tightening was for

`OVERFLOW_EPSILON` is **2** logical px now, down from 6. It was 6 to absorb the
quarter-em overshoot; that overshoot is gone, and 2 is what the remaining
rounding justifies — so every layout check in the workspace resolves three times
finer than it did.

Dropping it reported 21 cells of small **real** overflows the old allowance had
been covering, in `radar`, `preferences`, `quick-preferences` and `phototools`.
They are fixed rather than tolerated, and they were worth finding: one was every
slider in the viewer, hanging out of its track because there was no slider
widget and ten panels had each drawn their own. See
[[viewer-ui-rows-shorter-than-their-text]].

## Still to do

Offer both fixes upstream (they are ours-only for now, like the rest of the
fork), and the same question for `ImageMeasure`, which resolves its width the
same border-box way.
