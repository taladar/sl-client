---
id: server-world-link-sets
title: A real link-set model — link numbers, root and children
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [server-world-ecs-store, gridspec-object-link-delink]
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

## Measured (2026-10-11)

[[gridspec-object-link-delink]] measured the order with a script in every
prim of a set, on aditi and on the local OpenSim, and wrote it up in
`book/src/gridspec/building.md` § Linking. Three things above are not what
the grids do:

- **"The prim linked last is 2" is Second Life's rule alone.** There the
  prims a link brings are numbered from 2 on, in the order the link named
  them, a set that joins whole keeping its own order, and the prims the
  root already had come after. On OpenSim a prim keeps its number and a
  newcomer takes the next: `A B` with `C` linked in is `A C B` on Second
  Life and `A B C` on OpenSim. The acceptance check against "what the local
  OpenSim reports" holds only for a set linked in one message.
- **A root delinked alone is replaced by link number 2 on Second Life, and
  by no link number on OpenSim**, which takes the first of a dictionary of
  parts and renumbers the rest from it.
- A single prim reports 0 on both, as written.

The order itself is kept now: `sl-fake-grid/src/link_sets.rs` holds each
set's children by link number and, for the OpenSim flavour, the slots of
the part dictionary, both by flavour (`LinkPolicy`), and `object-link-delink`
holds both flavours to the measured orders through the delinks alone.
`link_sets::children_in_order` is what a link number is to be read
from. What is left here is everything a **script** sees: the `llGetLink*`
/ `llSetLink*` family and the `LINK_*` sentinels over that order,
`llCreateLink` / `llBreakLink` through `link_sets`' own `link` and
`delink`, seated avatars after the prims, and the order kept across a take
and a rez and a save and a restore — and its move onto the entity store.
The case's `MEASURED_LIVE` table (what the scripts said) is the
specification: this task is done when the fake grid's scripts say the same
and those rows move into `MEASURED`.
