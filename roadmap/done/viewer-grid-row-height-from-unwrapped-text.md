---
id: viewer-grid-row-height-from-unwrapped-text
title: Upstream bug — a max-width column came out as tall as its text on one line
topic: viewer
status: done
origin: split out of [[viewer-text-node-padding-measure]] while fixing the text
  measure (2026-09-14)
refs: [viewer-ui-test-harness, viewer-text-node-padding-measure]
---

Context: [context/viewer.md](../context/viewer.md).

A **column with a `max_width` and no definite width** was laid out as tall as
its text on a single line, while the text itself was finally laid out, and
wrapped, at the clamped width. It then hung out of the bottom of the column.

The measured shape, from the headless harness (15 px text, a column with
`max_width: 560`, inside a 1600 px grid):

| node | `size` | `content_size` |
| --- | --- | --- |
| the text | `(536, 18)` | `(508, 54)` |
| its column | `(560, 42)` | `(560, 78)` |

18 px is **one** line; the text wraps to **three** at the width it actually
gets.

It was filed as a grid track-sizing bug, because a hand-written grid reproduced
it and every `bevy_ui` root node is a grid item of an implicit viewport node
(`display: Grid`, `justify_items: Start`). **That hypothesis was wrong**: grid
track sizing is innocent, and the defect is two separate bugs in `taffy` 0.10.1
that each produce the same symptom.

## 1. Flexbox sized the column's items into its parent's width

Minimal repro, no Bevy: a taffy tree of `container > column(max_width 560) >
text leaf` whose measure returns more lines for less width.

A grid measures its items with **no known width**, only an available width, the
grid area: 1600 px. `flexbox::determine_available_space` passed that straight
on to the column's items. It never clamped it by the column's own `max_width`.
So a column container, whose main axis is its height, measured its text at
1600 px as one line and took that height. Only afterwards did it clamp its own
width to 560, and the final layout wrapped the text at 560 into three lines.

The fix clamps an unknown dimension by the container's own min/max size before
the content-box inset is subtracted. A box can never be wider than its
`max_width`, so its content can never have more room than that. Block layout
already gets this right, because it resolves its width before laying out
children, which is why a *block* column in the same grid was clean.

## 2. The measure cache matched a known width against an output width

The repro showed the symptom in a **flex row** parent too, which the flexbox
fix did not touch. The trace showed the reason. To size the column, the row
first measures it with no known width (text laid out at 1400 px, result clamped
to `(560, 18)`). It then asks for its height with a known width of 560. The
cache answered the second query from the first, because `Cache::get` also
accepts an entry whose **output** width equals the query's known width. That
equivalence is false whenever the output was clamped: laying a node out at a
known 560 is not the layout that happened to come out 560.

The fix matches only on the known dimensions an entry was computed with. That
is what upstream taffy #911 ("More correct caching logic") does. The cost is the
one upstream accepted: taffy's own `measure_count_*` tests go from 4 measures
to 7 through a 100-deep tree, the same edit upstream made to them. It is a
constant, not a growth with depth.

## Upstream status

Upstream `v0.14.0` was checked with the same repro. It has the cache half (the
flex-row case is clean there) but **not** the flexbox half: the grid case is
still one line tall. `bevy_ui` 0.19 pins taffy 0.10, so the fix lives in a fork
either way.

## What landed

- `github.com/taladar/taffy`, branch `sl-client-flex-max-cross-available` off
  `v0.10.1`: both fixes and two regression tests in `tests/measure.rs` (the
  column as a grid item and as a flex row item). Both tests fail without the
  fix, and taffy's full suite, including its 4277 Chrome-generated fixtures,
  passes with it.
- `[patch.crates-io] taffy` pinned to that rev, with the diff documented in
  the workspace `Cargo.toml`, and the fork in `deny.toml`'s `allow-git`.
- `sl-viewer-testkit`'s inverted canary
  `a_grid_row_takes_its_height_from_a_wrap_that_never_happens` is replaced by
  three positive tests: the column in a grid, in a flex row, and as a root node
  of its own (the original sighting).
