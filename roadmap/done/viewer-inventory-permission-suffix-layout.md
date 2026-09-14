---
id: viewer-inventory-permission-suffix-layout
title: An inventory row drifts left, and its permission suffix oscillates
topic: viewer
status: done
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

## Resolution

Both symptoms were one defect with two faces: **a row is a flex line, and three
of its parts were willing to give width back**. Only one may be — the label's
clip container, which shrinks to nothing and hides the tail of the name behind
the `…`. The fix declares `flex_shrink: 0.0` on the other two, and the row's
live structure moved out of `populate_new_rows` into `spawn_row_parts` so a
headless test can build the row the window actually draws.

**The drift** was the depth indent. It carried a width and the default
`flex_shrink: 1.0`, so a narrowing panel took the tree's indentation back first:
a depth-3 row laid its 48 px indent out at 24 px, and arrow, icon and name slid
left with it. Reproduced headlessly at every swept width, which is why this is
the half that never needed a live check.

**The oscillation** was the decoration. It could shrink *and* wrap, so
`(no copy) (no modify) (no transfer)` drew on three lines inside a 22 px row;
and because it shared the shrink with the label's clip, it broke the assumption
`ui_ellipsis` is built on. That module measures the name against "the clip's
width plus whatever the marker occupies", which is only the width the clip would
have *without* the marker while the clip is the sole giver. With the decoration
shrinking too, revealing the marker took part of its width out of the
decoration, the clip lost less than the marker's width, the name read as
fitting, the marker hid, and the row had two states and no resting one. The
decoration is now `no_wrap` and unshrinkable, so the clip is again the only part
that gives.

## The abbreviation

The three permissions are a **closed set** — copy, modify, transfer — with three
different initials, so `(no c) (no m) (no t)` distinguishes exactly what the
words do in a third of the room. Measured in the harness at the row font: the
words shape to **231 logical px** and the initials to **126**, against **83 px**
of fixed columns before the name. At the window's own default 340 px width the
words therefore leave the name **26 px** — and the initials leave it 131.

So `SuffixStyle` picks the spelling **for the whole panel** from the width the
list viewport was laid out at (`choose_suffix_style`): initials below
`SUFFIX_ABBREVIATION_WIDTH` (420 px — where the words stop leaving the name the
~100 px that makes an item recognisable), words above it plus a hysteresis band,
so a drag resting on the threshold does not respell every row on a jittered
pixel. The input is the *floater's* width, which no row can influence — a rule
that measured the decoration against the room left over would be the same
feedback loop again. `DisplayRow` accordingly holds `RowDecorations` (the facts:
link, withheld permissions, worn) rather than a finished string, and the text is
spelled at bind time.

The threshold is pinned to the font rather than to arithmetic:
`the_threshold_is_where_the_words_stop_leaving_a_name` shapes the real strings
and fails from **both** sides — too low, and the words crowd the name out; too
high, and rows are spelled short that had room for the words.

## The harness check this earned

`sl_viewer_testkit::stability_violations`, in `layout_violations` and so
retroactive over every registered element in every matrix cell: snapshot every
laid-out box, advance a frame, and report anything that moved, resized, appeared
or vanished. It is the check that a tree has *a* resting state rather than two,
which nothing in the suite asked before — every other check is satisfied by
whichever of the two frames it happens to look at. Its tolerance is sub-pixel,
not the `OVERFLOW_EPSILON` the box checks use: the same tree laid out twice has
nothing for a tolerance to absorb.

**Frame by frame, not `settle` twice** — and this is the part worth carrying
forward, because the obvious spelling (the one this task proposed) does not
work. A layout that flips *every* frame is back where it started after two, so
comparing a settled tree against itself two updates later is blind to precisely
the oscillation the check is for. The first draft was written that way and
passed a fixture built to flicker; it now compares consecutive frames, twice,
which sees a period of one and a period of two alike. A settled tree may be
compared frame to frame at all because that is what settled means: the
after-layout passes are write-guarded, so once they agree with the boxes they
read they write nothing. (`settle` needs two frames for a different reason —
reaching that state from a tree that was only just spawned.)

The check has its own teeth test (`a_node_that_flips_between_two_widths_is`
`_caught`): a node whose width is rewritten from its own laid-out width, which
is every feedback loop in this UI reduced to four lines.

Row-level regressions live with the row (`row_layout_tests`), because the
registry sample is deliberately *not* the live row — it sizes its columns to
content and takes a minimum rather than a fixed height so it survives the
sweep's font and pseudolocale axes, and that is precisely why the sweep had
nothing to say about either symptom.
