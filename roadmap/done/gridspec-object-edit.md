---
id: gridspec-object-edit
title: Object edits on each grid: transforms, shape, flags, admin fields, undo, duplicate
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-build-phantom-flag-wrong-bit,
  viewer-edit-permission-gating, viewer-build-general-sale-clickaction,
  server-fake-grid-edit-permission-enforcement, server-fake-grid-object-undo,
  test-conformance-object-edit-click-action-opensim, gridspec-groups,
  gridspec-aditi-test-land, gridspec-neighbours-crossing]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-10)

Measured and written up in `book/src/gridspec/building.md` § Editing and
§ Somebody else's object.

- **Discover.** `object-edit` rewritten as a census of one avatar and its
  own cubes: what a move, a turn, a resize, a material, a flag, a shape, a
  click action and the search flag are answered with; a side of a
  millimetre and of a kilometre; names of 254 bytes, with a pipe and a line
  break, not ASCII, and empty; every bit set on and cleared from each of
  the five masks; a group and a deed that are nobody's; what nine steps of
  undo and of redo restore after one edit of each kind, and how many moves
  an undo steps back out of; a duplicate by an offset and two on a ray, of
  a cube with a record and something in it; and a move far up, under the
  ground and off the region's edge. A new `object-edit-stranger` measures a
  second avatar's edits, takes, duplicates and undo of the first's cube,
  with no right over it, with move set for everyone and with copy set for
  everyone, and a deed by a plain member of a group. `object-edit` ran five
  times on aditi (Mauve) and six on OpenSim, `object-edit-stranger` three
  times on each.
- **Findings.** A move and a turn come back terse, everything else in
  full. OpenSim says nothing of a click action or the search flag, and
  drops a `MultipleObjectUpdate` stating a position, a rotation and a scale
  at once (which the old case sent). Neither grid takes an
  `ObjectCategory`. A side is kept within 0.01–64 m on Second Life and
  0.001–256 m on OpenSim, in silence. Both keep 63 bytes of a name and 127
  of a description; Second Life makes every non-ASCII byte and a pipe a
  question mark, and **OpenSim stores a hex dump of a name that is not
  ASCII**. An owner may clear a bit of its own owner mask on Second Life
  and not on OpenSim; everyone cannot be given modify on Second Life and
  can on OpenSim; a next-owner mask without copy keeps transfer on both,
  and move too on OpenSim. Second Life sends an object moved above its
  ceiling or off an edge with no neighbour to its owner's Lost And Found
  "because it went off-world", and refuses a move under the ground;
  OpenSim keeps an object at any height and refuses a move off the edge.
  **Undo is transforms only on both** — position, rotation, scale — twenty
  deep on Second Life and five on OpenSim. A duplicate copies the record,
  the contents and the creation date on both. Nothing a stranger sends
  changes an object on either grid, except that **on OpenSim a stranger's
  `Undo` steps back out of the owner's move**; Second Life refuses most
  things with an alert and an update that puts the object back, OpenSim
  mostly with nothing.
- **Fake grid.** `EditPolicy` (`imitates.rs`, eleven rows): the limits of a
  side, what a name is kept as, the mask rules, a deed's refusal, the undo
  depth, the quiet edits, the dropped combination, what a copy carries and
  what becomes of an object moved out of the world. For both flavours: a
  move and a turn are answered terse, an undo steps through placements and
  nothing else (it restored whole snapshots, names included, ten deep), a
  category is not taken, a duplicate copies the contents and names the
  agent as last owner, and a duplicate on a ray is answered (it was not).
  A group set and a deed do nothing, the fake grid having no groups; it
  used to obey both. `object-edit` runs offline on both flavours, held to
  a table of a hundred and sixty measured answers.
- **Viewer.** New `e2e_objects` test, on each fake flavour and on each live
  grid: a prim rezzed and named through the Build window is given a side
  of a kilometre and reads its grid's largest in the window and in the
  world; a move the grid refuses is put back in both; Ctrl+Z and Ctrl+Y
  step a move back and forward; and on Second Life a move off the edge
  takes the prim out of the world and shows its item in the inventory.
  It found one gap, fixed here: **a transform field that was committed
  with `Enter` kept the focus and so kept reading what was typed**,
  whatever the grid made of it — the sync skipped any focused field. It
  skips one only while it is being typed into now, and a field nothing was
  typed into no longer sends its row again on losing the focus, which had
  been costing the grid a step of undo for nothing
  (`sl-viewer-edit/src/edit_tool.rs`, `FieldSynced`; headless test in
  `build_floater_test`).
- **Also settled.** [[test-conformance-object-edit-click-action-opensim]]
  (the case's expectation was wrong) and [[server-fake-grid-object-undo]]
  (the history exists and is now the measured one).
  [[server-fake-grid-edit-permission-enforcement]] is unblocked, with
  `object-edit-stranger` as its specification.
- **Not done here.** The fake grid lets anybody edit anything
  ([[server-fake-grid-edit-permission-enforcement]]), so
  `object-edit-stranger` is live only. Not measured: where exactly Second
  Life's ceiling and floor are (only 5,000 m and 50 m below zero were
  sent; the fake grid draws its lines at 4,096 m and at zero); a move off
  an edge another region lies beyond ([[gridspec-neighbours-crossing]]
  measured an avatar's crossing, not an object's); a deed by a member who
  has the power, and an object's group with groups in the fake grid
  ([[gridspec-groups]]); an owner's friend with edit rights and an estate
  manager; whether an edit after an undo ends what a redo can reach; a
  duplicate of a linkset. The viewer's Phantom checkbox bug
  ([[viewer-build-phantom-flag-wrong-bit]]) is untouched.

## Known already

The fake grid applies every edit with no permission check; its undo
(depth 10, whole snapshots, properties too) diverges from OpenSim's
transform-only undo. `server-fake-grid-object-undo` predates the
implementation. Duplicate does not copy contents; `DuplicateObjectsOnRay`
is unhandled.

## Discover

Run `object-edit` on aditi; add legs for clamps (scale, position off-region),
non-owner refusal (second avatar), next-owner mask clamping, name / desc
truncation, deed without power, undo scope and depth, duplicate (contents,
ownership) and duplicate-on-ray.

## Document

`book/src/gridspec/building.md` § Editing.

## Fake grid

Small in this task: clamps, refusal silence, mask clamping, undo scope per
flavour, duplicate contents and on-ray. Permission enforcement:
[[server-fake-grid-edit-permission-enforcement]].

## Viewer

Read back clamped values; converge after a corrective update; `e2e` edits on
both flavours.
