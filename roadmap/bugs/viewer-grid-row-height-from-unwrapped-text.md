---
id: viewer-grid-row-height-from-unwrapped-text
title: Upstream bug — a grid row takes its height from a wrap that never happens
topic: viewer
status: bugs
origin: split out of [[viewer-text-node-padding-measure]] while fixing the text
  measure (2026-09-14)
refs: [viewer-ui-test-harness, viewer-text-node-padding-measure]
---

Context: [context/viewer.md](../context/viewer.md).

A **`Display::Grid` container with an auto row** sizes that row from a layout
performed at the item's *max-content* width, and never re-measures once the
item's real width is known. An item whose width is then clamped — by its own
`max_width`, or by the track — keeps the height it had when its text was one
unbroken line.

The measured shape, from the headless harness (15 px text, a column with
`max_width: 560`, inside a 1600 px grid):

| node | `size` | `content_size` |
| --- | --- | --- |
| the text | `(536, 18)` | `(508, 54)` |
| its column | `(560, 42)` | `(560, 78)` |

18 px is **one** line; the text wraps to **three** at the 536 px it actually
gets. The row is two lines short, and the text spills out of the bottom of it.

## Why it shows up without anyone writing a grid

**Every root node is a grid item.** `bevy_ui` parents each root to an implicit
viewport node of its own making — `display: Grid`, `align_items: Start`,
`justify_items: Start` (`bevy_ui-0.19.0/src/layout/ui_surface.rs`,
`get_or_insert_taffy_viewport_node`) — so a node spawned with no UI parent and
sized by its content lands on this whatever its own `display` is. That is why
the fixture that first showed it (a bounded panel spawned as its own root) fails
while the same tree under `UiRoot` is clean: under the root the panel's width is
definite from the start, so nothing is ever measured at max content.

## Not the text measure

This was found inside [[viewer-text-node-padding-measure]] and is **not** the
same defect: that one is the text measure wrapping at the wrong width, and it is
fixed. Controls, all on the fixed measure:

- the same tree under `UiRoot`, `max_width` and all — clean;
- the same tree under `UiRoot` with `align_self: Start`, so the column is
  shrink-to-fit and its width *is* clamped by `max_width` — clean;
- the same column as a root, with a definite `width` instead of a `max_width` —
  clean;
- the same column inside a hand-written `Display::Grid` under `UiRoot` —
  **reproduces exactly**, which is what says grid rather than root.

So the suspect is `taffy`'s grid track sizing (`taffy-0.10.1`,
`compute/grid`): a row track sized from an item's max-content contribution has
to be resolved *after* the item's inline size, which is the whole reason CSS
Grid sizes columns before rows.

## To do

Minimal `taffy` repro (no Bevy: a grid with one auto row, one item with
`max_width`, a leaf measure that returns a taller size for a narrower width) →
issue → fix or `[patch.crates-io]`, per
`sl-client-fork-upstream-for-upstream-bugs`.

The canary is `a_grid_row_takes_its_height_from_a_wrap_that_never_happens` in
`sl-viewer-testkit`'s `measure_tests`: it asserts the bug is **still present**,
so it starts failing the day this is fixed.
