---
id: gridspec-object-properties
title: Object properties and selection replies on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-asset-save-mutation-survey,
  gridspec-task-inventory, gridspec-object-rez-derez,
  gridspec-object-link-delink]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-09)

Measured and written up in `book/src/gridspec/objects.md` § Properties.

- **Discover.** `object-properties` rewritten as a two-avatar census: rez a
  cube, select it (again while selected, after a deselect, as the other
  avatar, by a local id the region does not have), ask for its family record,
  then drop an item into it, rename, re-describe, price and re-permission it
  three times over — both avatars selecting, the editor alone, nobody — and
  link a second cube under it. A new `object-select-scene` selects what a
  region already holds: roots, child prims, a neighbouring region's prims
  and the agent's own avatar. Six and two runs on aditi (Mauve; Ahern),
  eight and two on OpenSim.
- **Findings.** The survey's "aditi never answers an `ObjectSelect`" is
  wrong: both grids answer every select of an object, every time, with the
  record and with its physics record over the event queue, and neither
  answers a select of an avatar or of an unknown local id. A select of a
  root answers for the root alone; a family request about a child answers
  with the root. The creation date is in microseconds on both. An edit of
  the record is told to its editor and to nobody else — Second Life every
  edit, OpenSim a price or a permission but not a rename — so another avatar
  holding the prim selected is never told. A write into a prim's
  **contents** is different: Second Life sends the record to every session
  holding the prim selected (the writer only if it is one), OpenSim to the
  writer alone. Second Life answers a deselect with a terse update; OpenSim
  sends somebody else's object again in full on a select. A link sends the
  linker the children's records on Second Life and the root's on OpenSim.
  The two grids' new-prim records differ in masks, last owner, ownership
  cost and texture ids.
- **Fake grid.** `PropertiesPolicy` (`imitates.rs`, six rows): who a rename
  and a contents write are told to, what a select and a deselect bring,
  whose record a link sends, a child's sale state, a new prim's record. For
  both flavours: no edit is published to other selectors any more (it went
  to every selector, which neither grid does), a select brings the physics
  record, a family request about a child answers with the root, a child's
  record carries the root's masks, the creation date is in microseconds.
  Both cases run offline on both flavours, held to a table of sixty
  measured answers.
- **Viewer.** The Build window's name, description and permission boxes
  stay shut until the record has come (they were live over a blank record).
  New `e2e_objects` tests: on each fake flavour a rezzed
  prim's record opens and fills the General tab, a rename holds whether
  the grid answers it or not, and an unanswered select leaves the name
  blank and shut; the same rez, record, rename and fresh read on each live
  grid (`SL_E2E_GRID=opensim` and `=aditi`, both run).
- **Harness.** A case that rezzes recognised "the next unseen object of
  ours" in any region: on OpenSim a neighbour's fixture, arriving late down
  its child circuit, was taken for the new cube once.
  `wait_for_own_new_object` now takes an object of the agent's own region
  only.
- **Not done here.** The fake grid does not imitate how many records go to
  a message ([[server-fake-grid-object-record-batching]], deferred until it
  matters to a test), Second Life's texture ids in a record
  ([[server-fake-grid-object-record-texture-ids]]), or its price of 10 on a
  prim that is not for sale. Not measured: what a group or owner change is
  answered with, a category edit's answer, and who is told when an asset in
  a prim is saved over ([[test-asset-save-mutation-survey]]).

## Known already

On aditi an `ObjectSelect` reportedly never returned `ObjectProperties`; the
fake grid answers and pushes changes to selectors.

## Discover

Run `object-properties` on aditi; a second avatar for the push-to-other-selector
half; record field contents.

## Document

`book/src/gridspec/objects.md` § Properties.

## Fake grid

Small — per-flavour answers in this task (unless SL truly stays silent).

## Viewer

The build floater must not hang on a slow / silent select.
