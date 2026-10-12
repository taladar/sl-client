---
id: viewer-touch-cursor
title: Show by the cursor that a prim takes a touch or a click action
topic: viewer
status: ready
origin: gridspec-touch-grab (2026-10-12)
refs: [gridspec-touch-grab, viewer-left-click-click-actions]
---

Context: [context/viewer.md](../context/viewer.md).

Nothing tells the user a prim is worth clicking: the cursor is the arrow
over everything but a media face. The reference
(`LLToolPie::handleHover`) shows the hand over a prim whose script takes
touches — `FLAGS_HANDLE_TOUCH` on it **or on its root**, which is what
both grids need read (Second Life flags the children of a touch root,
OpenSim does not; `book/src/gridspec/building.md` § Touch and grab) — the
grab cursor over a physical one, and a cursor of its own for each click
action (sit, buy, pay, open, play, zoom).

The hover pick exists (`PickPurpose::Hover`, the tooltip's); the cursor
writers are `media_prim.rs`, `camera.rs` and `edit_create.rs`, which have
to keep precedence.

Acceptance: a fixture test that hovering a touch-flagged prim, a child of
a touch-flagged root and a plain prim sets the hand, the hand and the
arrow.
