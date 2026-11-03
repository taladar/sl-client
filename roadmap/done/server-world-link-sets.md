---
id: server-world-link-sets
title: A real link-set model — link numbers, root and children
topic: server
status: done
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

## From server-world-sit-and-attach (2026-11-02)

`llGetLinkNumber` and `llGetLinkKey` are implemented over `Host::links`, whose
`Links` now carries each prim's key and the sitters after the prims in sit
order; a lone prim with a sitter is link 1. The rest of the family is still
this task's.

## Done (2026-11-03)

What a script sees of a linkset, over the order `link_sets` already kept:

- **The order on the entity store.** `scene_store::LinkOrder` is a component of
  each root — its children by link number and, for the OpenSim flavour, its part
  collection's slots — so a root that leaves takes it and a copy of the region
  (`ObjectStore`'s `Clone`, a scenario copied into its region) keeps it.
  `SceneFixtures::links` is gone. A take files a set root first in link order
  and a rez pushes it back in that order, which is what `children_in_order`
  falls back on (`a_taken_linkset_comes_back_in_its_link_order`,
  `a_copy_of_the_region_keeps_the_order`); a restart leaves the prims as they
  are.
- **The runtime's `Links`.** Each `LinkedPrim` carries its name and each
  sitter is a `LinkedAvatar` (key and legacy name). `Links::numbered` is the
  one resolution of a single link number (`LINK_THIS`, a lone prim's 0, the
  prims, then the sitters; `LSL_Api.GetLinkEntity`'s rule, which both grids
  follow for `llGetLinkName`), and `Links::named` the one resolution of the
  `LINK_*` sets, which `llMessageLinked` now goes through and the
  `llSetLink*` family is to.
- **Functions.** `llGetLinkName`, `llGetObjectName` (`library/sitting.rs`),
  `llGetLinkKey` rewritten over `Links::numbered`; `llCreateLink`,
  `llBreakLink` and `llBreakAllLinks` (`library/linking.rs`) behind the new
  `PERMISSION_CHANGE_LINKS`, refused in each grid's words (`prim/link` and
  the new `prim/link-refusals`, run on aditi, YEngine and XEngine; the wiki's
  Second Life delink wording turned out wrong), through three new
  `Host` methods (`create_link`, `break_link`, `break_all_links`).
- **The region.** `RegionHost` validates a link target (a prim of the region,
  not worn, the same owner) and defers the link or delink as an `Effect`
  applied after the tick through `link_sets::link` / `delink`, the same calls
  a viewer's `ObjectLink` / `ObjectDelink` make, and publishes the changed
  sets whole under `REGION_SOURCE`.
- **`CHANGED_LINK`** on a viewer's link and delink (every prim of the sets
  touched) and on a script's, per `LinkPolicy::events` (`LinkEvents`): on
  OpenSim a newcomer first hears its own 32768, `llBreakLink` of a child
  tells the child alone (the recorded OpenSim run; the book's row said "the
  same" and is corrected), `llBreakAllLinks` tells the root once per child,
  `llBreakLink` stands the sitters up first on both grids, and
  `llBreakAllLinks` does on Second Life alone (`prim/link-refusals`). A
  scripted link too far is refused silently on Second Life (the same probe);
  OpenSim has no distance limit. Unmeasured still: the refusal of a scripted
  link that would make too many prims (the probe would need 256), heard as
  nothing.

Acceptance: `object-link-delink`'s `MEASURED_LIVE` rows
(`a_single_prim_is_link_number`, `numbers_linked_at_once`,
`numbers_with_a_prim_added`, `numbers_of_two_sets_joined`,
`numbers_once_the_root_left`, `a_root_delinked_alone_promotes`) moved into
`MEASURED`, and the case plants its reporting scripts on both fake flavours and
passes offline — the live OpenSim numbers the check was to be held against are
those rows, measured there on 2026-10-11. `sl-fake-grid/tests/scripts.rs`'s
`a_scripted_linkset` runs a three-prim set whose root reports `1/3: A B C`,
pings exactly the two children with `LINK_ALL_OTHERS`, and, granted, breaks link
2, links it back (as 2 on Second Life, last on OpenSim) and breaks every link,
each prim hearing what its grid tells it; a lone prim reads link 0 (runtime
tests in `vm/tests/linking.rs`).

Not done here, and left to [[server-lsl-lib-prim-state]], which lists them
already: the parameter half — `llGetLinkPrimitiveParams`,
`llSetLinkPrimitiveParams(Fast)`, `llSetLinkColor`, `llSetLinkAlpha`,
`llSetLinkTexture` — which need the `PRIM_*` codec and the update plumbing of
that tranche; they resolve their link argument through `Links::named`. The
`prim/link` probe replays only once they exist.
