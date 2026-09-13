---
id: viewer-inventory-permission-suffix-layout
title: An inventory row drifts left, and its permission suffix oscillates
topic: viewer
status: bugs
origin: user report (2026-09-13)
refs: [viewer-inventory-folder-tree, viewer-ui-text-foundation]
---

Context: [context/viewer.md](../context/viewer.md).

An inventory row that carries a permission suffix — the
`(no copy) (no modify) (no transfer)` after the item's name — misbehaves as the
inventory panel is narrowed past the width that fits name and suffix on one
line. **Two distinct symptoms**, reported as such (2026-09-13):

- **the whole row, name included, drifts left** — the row's content moving out
  from under its own left edge as the panel narrows, rather than being clipped
  or ellipsised at the right;
- **the suffix alone oscillates** between two states at certain widths, flipping
  rather than settling.

They are worth chasing separately, because the shapes differ.

The **drift** is a row whose content is wider than the box and is being
positioned against the content rather than the box: the leftward direction says
whatever centres or end-aligns the row is resolving against the overflowing
width. Look at the row's alignment and at whether its parts are in a wrapping
container (the reference draws the suffix as part of the name's text, not as a
sibling that can be pushed).

The **oscillation** is a **feedback loop** — a layout whose output changes the
measurement it was produced from. The candidate in this UI is the reveal /
ellipsis pass (`ui_ellipsis`): it measures a row, shortens the text to fit, and
so changes what it measured. A flip between exactly two states is what that
looks like when the shortened text fits and the full text does not.

Reproduce by dragging the inventory floater's edge slowly through the width
where the name and the suffix stop fitting, and watch one row.

A headless regression is cheap once the cause is known: the layout harness
sweeps an element at several widths, and an oscillation shows up as a layout
that differs between two settles of the **same** tree — which is worth adding
as a check in its own right (`settle` twice, assert the boxes match), because
nothing in the suite would currently catch a flip-flopping layout.
