---
id: gridspec-object-link-delink
title: Link order, limits and refusals on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-object-properties, server-world-link-sets,
  server-fake-grid-edit-permission-enforcement,
  viewer-link-limit-from-open-region-settings,
  protocol-variable-block-lists-over-255]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-11)

Measured and written up in `book/src/gridspec/building.md` § Linking.

- **Discover.** `object-link-delink` rewritten as a census of one avatar
  and its own cubes, each carrying a script that says its link number and
  its set's names in order whenever its links change: what a link and a
  delink are answered with, selected and not; the order of a set linked at
  once, of one a prim was added to and of two sets joined, and which prim
  takes over when a root is delinked alone; a link naming a child, and one
  prim; a prim the owner took modify off; how far apart two cubes link, to
  the metre, at two sizes, and three in a line; a set doubled to 256 prims
  and one more; a set of 32 and of 64 made physical. A new
  `object-link-stranger` has a second avatar link and delink the first's
  prims and each link a prim of the other's. `object-link-delink` ran four
  times on aditi (Mauve) and four on OpenSim, and once more on aditi with
  three cubes in a line in all six orders at four spacings;
  `object-link-stranger` twice on each.
- **Findings.** Both grids send every prim of a changed set again in full.
  **Second Life numbers the prims a link brings from 2 on, ahead of the
  ones the root had; OpenSim numbers them last.** A root delinked alone is
  replaced by link number 2 on Second Life and, on OpenSim, by the first of
  a dictionary of parts — number 2, 3 and 4 in three sets — whose order
  the case's whole history of links decides. Second Life refuses, whole
  and with an alert, a link whose prims would stand more than 64 m apart
  (whichever is the root, and whatever their size), a 257th prim, a prim
  whose owner mask lacks modify, and more than 32 prims made physical;
  OpenSim refuses none of them. **Second Life does not send the same alert
  twice within a few seconds**: of refusals six seconds apart every other
  one was silent. Across two owners nothing links on either grid, with
  `LinkFailedOwnersDiffer` or `LinkFailedNoPermToEdit` on Second Life and
  nothing on OpenSim.
- **A correction.** [[gridspec-object-properties]] had Second Life sending
  a link's children's records. That was the answer to the rename its case
  sent just before the link. A link sends the root's record: on Second
  Life to a linker holding the root selected, on OpenSim always. The case
  now hears the rename out first, and its table, the book and the fake
  grid say so.
- **Fake grid.** `LinkPolicy` (`imitates.rs`, eight rows) and a new
  `link_sets` module that keeps each set's order — and, for the OpenSim
  flavour, the dictionary slots that decide a promotion. A link that names
  a child brings its set (it hung the child alone off the new root), a
  root delinked alone leaves the rest a set (it freed them all), a
  duplicate copies a whole set (it copied the root), every prim of a
  changed set is sent again in the flavour's order, and the Second-Life
  flavour refuses what Second Life refuses, with its alerts and their
  parameters. A derez of more than 255 prims is killed in more than one
  message (it failed to encode,
  [[protocol-variable-block-lists-over-255]]). `object-link-delink` runs
  offline on both flavours, held to sixty-seven measured answers.
- **Viewer.** New `e2e_objects` test, on each fake flavour and on each
  live grid: two prims rezzed through the Build window, one moved a
  hundred metres up, both selected and linked by the chord. On Second Life
  the grid's refusal is shown in its own words — `LinkFailed` has no
  template in any viewer — and both prims are drawn free; on OpenSim one
  is drawn hanging off the other. It passes on both fake flavours, on
  aditi and on the live OpenSim (the failures it met there at first were
  the harness's, since fixed:
  [[test-e2e-objects-live-opensim-intermittent]]). No gap in the viewer's
  linking was found.
- **Not done here.** A script on the fake grid cannot read a link number:
  the order is kept, the `llGetLink*` family over it is
  [[server-world-link-sets]], which now has the measured rules and whose
  acceptance is the rows of this case only a live grid is held to. The
  fake grid lets a stranger link anything
  ([[server-fake-grid-edit-permission-enforcement]]), so
  `object-link-stranger` is live only. Our viewer refuses a 257th prim
  itself where a stock OpenSim would take it
  ([[viewer-link-limit-from-open-region-settings]]). Not measured: whether
  Second Life's reach is every pair's or a box's, and whether it grows
  with prims larger than ten metres; whether a set's order survives a take
  and a rez; where a seated avatar is numbered; a link of an attachment,
  of prims across a region's border, of a no-copy prim with a no-transfer
  one, and by a friend with edit rights; whether Second Life's 32 for a
  physical set is a count or a cost.

## Known already

First id is the root; OpenSim links same-owner only. SL link order ("the prim
linked last is 2") and limits are unmeasured.

Measured with the record ([[gridspec-object-properties]], 2026-10-09): a link
sends the linker the record of each child on Second Life and the root's on
OpenSim; a child's record carries the root's permission masks on both and the
root's sale state on OpenSim only; a select of the root answers for the root
alone.

## Discover

Run `object-link-delink` on aditi; probes for link order, too-far / too-many
prims and their refusal, delink of the root.

## Document

`book/src/gridspec/building.md` § Linking.

## Fake grid

Large — [[server-world-link-sets]].

## Viewer

The link refusal alert; chords via `e2e_live_checks` live.
