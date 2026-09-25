---
id: viewer-rlv-console-lines-wrap
title: The RLVa console cuts a long line behind an ellipsis where the reference wraps it
topic: viewer
status: ready
origin: viewer-gallery-floaters-are-mostly-stubs (2026-09-25)
points: 2
refs: [viewer-gallery-floaters-are-mostly-stubs]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

The console's transcript is a `VirtualList` of fixed-height rows. Its lines
used to wrap anyway and spill over their neighbours; the layout sweep caught
that, and the fix made each line one row, cut at the window's edge behind the
locale ellipsis. That is legible, but the reference console wraps a long
line, and an RLV command is often longer than the window.

## What to do

The transcript holds at most 512 lines, so it does not need virtualising: a
plain scrolling column of wrapping lines (the Conversations transcript's
shape) shows every line whole. Swap the list for that, keeping the line
builder (`spawn_console_row`) and its classes.

## Done when

A long console line is shown whole, wrapped, and the sweep still passes.
