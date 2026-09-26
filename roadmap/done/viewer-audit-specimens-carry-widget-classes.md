---
id: viewer-audit-specimens-carry-widget-classes
title: A gallery specimen that hand-rolls its nodes shows none of the skin
topic: viewer
status: done
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

## Implementation (2026-09-26)

All 60 `ELEMENTS` specimens were audited against the live builder each stands
for. 49 carried everything their live widget does (most by calling its
builder). The gaps:

- **`inventory-row`**: a hand-made copy of the tree row with no `sk-list-row`,
  no `Pickable`, no `sk-list-surface` and no `sk-folder-label` — the emoji
  picker's bug again. **Deleted.** The `inventory` floater specimen already
  dresses its rows through the live `dress_tree_row` + `bind_row`, and the
  floater sweep runs it through the whole matrix.
- **`build-create`**: a copy with the hint wearing `sk-build-value` where the
  live hint wears `sk-build-label`. Now calls the live `spawn_create_panel`,
  with the panel and tree-species row shown. The now-dead `PrimType::gallery`
  literals went with it.
- **`parcel-audio-bar`**: a copy missing the wrapper and two `Pickable`s. The
  live system's body is now `spawn_parcel_audio_cluster`, which both call.
- **`field-grid`** and **`text-editor`** (no live widget of their own): the
  cells and the editor wear `sk-field`, the grid's row labels
  `sk-build-label`, the editor drops the `sk-text` the live field does not
  carry.
- **`radial-menu-target`**: its prose is `text_role`d, and its address table
  wears the sub-pie caption class whose colour it already used.
- **`floater`** and **`bottom-toolbar`**: state classes a runtime system adds
  never reached the static specimen. The floater specimen is dressed as the
  front-most window (`sk-frontmost` / `sk-frontmost-text`); the toolbar
  specimen gains a Conversations button pulsing `sk-attention`.
- Left as is, deliberately: `search-field`'s missing placeholder (documented —
  the harness would read it as overflow), and `label` / `panel`, which have no
  live counterpart.

**The test**: `ui_test::every_specimen_carries_its_live_widgets_classes`, a
table of (element, classes) pairs, each asserted present in the spawned tree.

### What the test found in the harness

Its first run said `experiences-floater` carried no table row. The element
sweeps built their apps with no string table, so every specimen built by a live
builder was measured with blank `Translated` labels, and one that fills itself
through a `Translator` (the experiences list) failed parameter validation and
drew nothing — logged, and invisible to the sweep. The floater sweeps already
installed English plus the cell's transform and the list widgets; the element
sweeps now do the same (`ui_test`'s own `spawn_element`).

### And what the honest labels found

With real pseudolocalised strings at 720 px, the narrow-window sweep reported
the Preferences alerts list as never settling. Measured frame by frame it does
settle, one frame after `settle`'s two: the list's scrollbar appears, which
narrows the rows, which tips a cell's value into overflow, which reveals its
`…`. A converging chain, not an oscillation. The element sweep now settles
until still (testkit `settle_until_still`, bounded), so a layout that never
comes to rest still fails the stability check.

Chasing it turned up a real reveal defect on the way: the `…` marker sits behind
the container's `column_gap` (table cells and inventory rows both), and showing
it costs the value's clip the marker **and** the gap, while
`apply_reveal_ellipsis` added back only the marker. A value in that 4 px band
kept whichever state it was first drawn in — the
`viewer-audit-ellipsis-reveal-latch` latch, narrower. The occupied width is now
what the clip gave up (`ui_ellipsis::marker_occupies`: the span of clip and
marker less the clip).

## Closed (2026-09-26)

Every registered specimen now carries its live widget's classes, and
`ui_test::every_specimen_carries_its_live_widgets_classes` fails the next one
that drifts.
