---
id: viewer-pie-label-width-alternates
title: A pie label's width alternates by a pixel, every frame, forever
topic: viewer
status: done
origin: found by the stability check added for
  [[viewer-inventory-permission-suffix-layout]] (2026-09-14)
refs: [viewer-pie-wire-ready-placeholders, viewer-inventory-permission-suffix-layout]
---

Context: [context/viewer.md](../context/viewer.md).

The pie menu's east label was laid out **153 physical pixels wide on one frame
and 154 on the next**, for as long as the menu was open. Not a transient: the
harness watched eight consecutive frames and the width alternated on every one
of them. Its position never moved — only its right edge, by one pixel.

Found by `sl_viewer_testkit::stability_violations` the moment that check went
into `layout_violations`, which is the entire argument for having written it:
nothing else in the suite asks whether a settled tree *stays* settled, so this
had been shipping under a test that measured it on whichever frame it happened
to land on.

## Cause

`fit_pie_layout` places each label by polar coordinate, offsetting it back by
its own half-extent — and read that extent from `ComputedNode::size`, which is
the box **rounded to whole physical pixels**. Which way it rounds depends on
where the node sits: a node whose left edge falls on a fraction can round a
pixel wider than the same node placed on a whole one.

That closes a loop. The placement moves the label, the move re-rounds its
width, the new width moves it back. The two states differ by the one pixel the
rounding can go either way on, and neither is a fixed point.

It is the same shape as the inventory row's flipping `…` marker
([[viewer-inventory-permission-suffix-layout]]) in a different widget: a pass
that reads the layout's output and writes something the next layout reads.

## Fix

Read `ComputedNode::unrounded_size` instead (`half_extent`). That is what the
text measured to, which is the same answer wherever the label is placed — so
the placement has a fixed point to reach, and reaches it. The label now holds
153 px across every frame, and the existing long-label sweep covers it, since
that test runs the whole of `layout_violations`.
