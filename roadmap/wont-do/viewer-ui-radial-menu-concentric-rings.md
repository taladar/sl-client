---
id: viewer-ui-radial-menu-concentric-rings
title: Concentric-ring pie menu (angle picks direction, distance picks ring)
topic: viewer
status: wont-do
origin: split from viewer-ui-radial-menu (2026-07); the "prototype alongside" option in that task, declined after the eight-slice widget landed
refs: [viewer-ui-radial-menu, viewer-object-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

**Won't do for now — a distinct design, not a gap in the pie widget.**

`viewer-ui-radial-menu` names concentric rings as the one option worth
prototyping that *raises* the eight-slot budget rather than nesting it: angle
picks the direction, distance from the centre picks the ring, in one
uninterrupted gesture with no paging. Angular stability survives, because a
direction still always means the same family, and it composes with named
sub-pies rather than competing with them.

It was **not** built. The eight-slice widget with named sub-pies is the
deliverable and it is complete; concentric rings are an additive second input
axis on top of it, with their own design cost that the delivered widget does not
need in order to be correct:

- **The label layout has no obvious home for a second ring.** The delivered
  widget places each label on its wedge at a single label-ring radius, and
  `fit_pie_layout` grows that radius so labels never overlap. A second ring of
  eight labels at a larger radius would have to interleave with the first
  without either ring's labels colliding — a real layout problem, not a tweak to
  the existing one.
- **Distance is currently free for the flick gesture.** The pie deliberately has
  **no outer bound while flicking** — a fast flick in a direction lands whatever
  distance it travels, which is half of what makes it fast. Concentric rings
  spend that freedom: distance would become significant, so a flick could no
  longer be "any distance in a direction". That is a genuine trade against the
  muscle memory the whole widget exists for, and worth deciding deliberately
  rather than bolting on.
- **Nothing needs it yet.** The eight-slot budget with *named* sub-pies (never
  `More >`) is enough for the object / avatar / land menus
  ([[viewer-object-context-menu]]); nesting by meaning is the answer the task
  settles on. Concentric rings are the alternative for when eight-plus-nesting
  proves too deep in practice — which is a judgement to make against real menus,
  not before they exist.

Revisit if a real domain menu turns out to nest uncomfortably deep and a ring
would flatten it *without* costing the flick gesture. The recursive sub-pie
mechanism and the angle maths are already in place, so this would be an additive
input-axis change, not a rewrite.
