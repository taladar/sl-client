---
id: viewer-audit-specimens-carry-widget-classes
title: A gallery specimen that hand-rolls its nodes shows none of the skin
topic: viewer
status: ready
origin: viewer-skin-list-row-striping live look (2026-09-23)
points: 3
refs: [viewer-ui-gallery-tab-order, viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

The gallery is where a skin author checks a palette, and several element cards
**hand-roll** their nodes rather than calling the widget's own spawn helper.
Those copies carry the geometry but not the `ClassList`, so the card shows the
widget's *shape* in none of the skin's colours — and a skin author reads that
as "the skin does not reach this widget", which may or may not be true of the
live one.

Found the hard way: `spawn_emoji_picker_specimen` built its own cells with no
class and no `Pickable`, so `.sk-tile:hover` could not select them. The card
was reported as "hover does nothing" while the live floater's cells were fine —
the specimen and the widget had drifted, and the specimen is what gets looked
at. Fixed there; nothing says the rest are right.

## What to do

- Walk `ELEMENTS` and, for each `spawn` that does not delegate to the widget's
  own helper, check that every node it builds carries the class the live widget
  puts there. Prefer *deleting* the hand-rolled copy in favour of the helper
  wherever the specimen does not genuinely need to be static.
- Where a specimen must stay static (no observers, so a sweep cannot click it),
  it should still carry the classes: a class is a look, not a behaviour.
- Consider a test: for a named set of (element, class) pairs, assert the
  spawned tree contains a node carrying that class. The sweep already spawns
  every element, so this is a filter over an existing harness rather than a new
  one.

## Done when

Every specimen paints what its live widget paints, and a class the live widget
carries but its specimen does not fails a test.
