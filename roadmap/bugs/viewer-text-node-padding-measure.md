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

`crate::ui`'s demo now puts the decoration on a **container** and leaves the
`Text` a plain child, which makes `size` and `content_size` agree. That is the
better structure regardless — a text run is not a box — so the workaround is not
a wart, and the convention is worth keeping even after this is fixed.

## To do

Confirm against `bevy_ui` upstream and report (or fix) it. Per
`sl-client-fork-upstream-for-upstream-bugs` the shape is: minimal repro → issue
→ fix → `[patch.crates-io]` until it lands.

**This needs [[viewer-ui-test-harness]] first.** A credible upstream report
needs a minimal repro that does not involve logging into a Second Life grid, and
the only reason this is written up from hand-measured numbers rather than a
failing test is that there is currently no way to run `bevy_ui` layout
headlessly from this workspace (`bevy_ui`'s own headless layout harness is
`pub(crate)`). The harness task is where that gets solved; this is its first
customer.

Suspect: `bevy_ui-0.19.0/src/widget/text.rs` — `TextMeasure::measure`, and how
`MeasureArgs::available_width` relates to the node's content box for a node that
has non-zero padding / border of its own.
