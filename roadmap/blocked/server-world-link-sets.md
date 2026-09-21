---
id: server-world-link-sets
title: A real link-set model — link numbers, root and children
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [server-world-ecs-store]
refs: [server-lsl-lib-prim-state, server-world-touch-and-grab]
---

Context: [context/lsl.md](../context/lsl.md).

An `Object` carries `parent_id: RegionLocalObjectId`, and that is the
whole of the fake grid's link-set model. `object_edits` handles
`ObjectsLinked` / `ObjectsDelinked` by rewriting that field. Nothing
knows what a **link number** is.

A link number is not derivable from `parent_id` alone: it is the root's
`1`, the children in the order the linkset was built, and — when an
avatar sits on the linkset — the seated avatars after the prims. Nearly
the whole prim-parameter half of the library is addressed by it:
`llGetLinkNumber`, `llGetLinkKey`, `llGetLinkName`,
`llGetLinkPrimitiveParams`, `llSetLinkPrimitiveParamsFast`,
`llSetLinkTexture`, `llSetLinkAlpha`, `llSetLinkColor`,
`llMessageLinked` with its `LINK_SET` / `LINK_ALL_OTHERS` /
`LINK_ALL_CHILDREN` / `LINK_THIS` / `LINK_ROOT` sentinels,
`llGetNumberOfPrims`, `llDetectedLinkNumber`, and the whole of
`llBreakLink` / `llCreateLink` / `llBreakAllLinks`.

Wanted, on top of the entity store:

- a link-set component on the root holding its children **in link
  order**, with the order preserved across a save/restore and a
  take/rez, and the link number of a child derived from it in constant
  time;
- the order rule stated and tested: the root is 1, the prim linked last
  is 2 (Second Life's ordering, which is the reverse of what a reader
  expects), and seated avatars occupy numbers after the last prim in sit
  order;
- a single-prim object's `llGetLinkNumber` returning **0**, not 1 — the
  special case content branches on;
- the `LINK_*` sentinel resolution in one place, so each library
  function that takes a link number resolves it identically;
- `llCreateLink` / `llBreakLink` acting on the same structure the
  viewer's link and delink edit already produces, so the two paths cannot
  disagree about what a linkset is.

Acceptance: a three-prim linkset reports link numbers 1, 2, 3 matching
what the local OpenSim reports for the same build order (check it with
`sl-repl` against a linked prim there); `LINK_ALL_OTHERS` from the root
of a three-prim set names exactly two prims; and a single prim reports 0.
