---
id: gridspec-task-inventory
title: Task inventory reads and writes on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-object-properties,
  test-phase-z-deferred-04,
  viewer-task-inventory-open-and-save-back, test-asset-save-mutation-survey,
  protocol-request-task-inventory-cap, protocol-cap-get-metadata,
  server-fake-grid-edit-permission-enforcement, gridspec-inventory-mutations,
  viewer-gesture-management-ui, viewer-material-save-to-object,
  server-lsl-lib-task-inventory]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-12)

Measured and written up in `book/src/gridspec/building.md` § Contents.

- **Discover.** `task-inventory` rewritten as a census of one avatar's cube
  with a second avatar standing by: an empty prim over UDP and over the
  `RequestTaskInventory` capability, the capability asked with the serial
  held, with another, about a prim nobody has and about nothing; a drop,
  and the same item dropped again; a script made new, made stopped, and
  dropped out of the inventory, and whether each runs; a script made, an
  item dropped, an item removed and an item moved out, each three times —
  with both avatars holding the cube selected, with the writer alone and
  with nobody; a remove and a move of an item the prim does not hold, a
  drop and a script naming an item the agent does not; a script's
  experience over `GetMetadata`; and a gesture and a material saved into
  the prim over their capabilities. A new `task-inventory-stranger` has a
  second avatar list the first's prim and drop into it, make a script in
  it, and move and remove what is in it. `task-inventory` ran five times on
  aditi (Mauve) and five on OpenSim; `task-inventory-stranger` twice on
  each.
- **Findings.** **Second Life sends the prim again in full to every viewer
  after every write** but a move, and the record with the new serial to
  the sessions holding it selected; OpenSim sends the record to the writer
  alone and the prim again only when a script lands or something is
  removed. **A script that starts moves Second Life's serial a second
  time**, told to the selecting sessions with a `ReplyTaskInventory` nobody
  asked for; a script asked not to run is left stopped on Second Life and
  run anyway on OpenSim. A second item of one name is `Name 1` on both. A
  move leaves a copyable item, and the serial, where they were on both.
  **OpenSim's remove files a copy in the agent's Trash.** Second Life's
  capability answers `304` for the serial held and `404` for a prim it
  does not have, names the types by their short names, and agrees with the
  UDP listing field for field. A write that cannot land is answered with
  nothing on both grids, but for Second Life's alert for a drop of an item
  the agent does not hold. A stranger is listed the prim on Second Life,
  without asset ids, and refused each write with an alert of its own;
  OpenSim neither lists nor says anything.
- **Capabilities.** [[protocol-request-task-inventory-cap]] and
  [[protocol-cap-get-metadata]], done here. `UpdateGestureTaskInventory`
  (both grids) and `UpdateMaterialTaskInventory` (Second Life) are
  requested, served by the fake grid and measured saving an item in a prim;
  their editors stay [[viewer-gesture-management-ui]] and
  [[viewer-material-save-to-object]].
- **Fake grid.** `ContentsPolicy` (`imitates.rs`, eight rows) and a new
  `task_contents` module: `RezScript`, a remove and a move, which were
  dropped; the serial's steps; who is sent the prim, its record and the
  unasked reply; OpenSim's Trash copy and its `Gesture updated`; Second
  Life's alert for a drop of an unheld item; and each script's run state,
  for `GetScriptRunning`. `RequestTaskInventory` and `GetMetadata` are
  served on the Second-Life flavour (`CapsDispatch` hands them to the
  driver, whose world the prims are). `task-inventory` runs offline on both
  flavours, held to ninety measured answers.
- **Viewer.** The Content tab reads a prim over the capability or UDP as
  the region allows. It no longer takes the first listing after a write of
  its own for the verdict when that listing is at the old serial — on
  Second Life the write and the listing travel separately — but asks once
  more, as the reference does. It re-reads on an unasked reply. A listing
  nobody answers now ends (`Event::TaskInventoryUnanswered`) and a failed
  capability falls back to UDP; either used to leave the tab saying
  "…adding" for good. New Script makes a running script, as the reference
  does (it asked for a stopped one). The script editor asks a task
  script's experience, shows it, and names it again in the save. New
  `e2e_objects` test, on each fake flavour and on each live grid: two
  scripts made from the Content tab, the second shown under the grid's
  name for it, one removed; on the fake grid's Second-Life flavour a
  script the grid says is under an experience, whose editor says so and
  whose save names it; and on both flavours a script made in a prim the
  grid has lost, which nothing answers.
- **Not done here.** The fake grid lets a stranger write anything
  ([[server-fake-grid-edit-permission-enforcement]], which now has the
  measured refusals), so `task-inventory-stranger` is live only. It runs no
  script: a script's `llGetInventory*` over these contents is
  [[server-lsl-lib-task-inventory]]. It gives an item made in the
  inventory Second Life's masks on both flavours and a script an asset
  from the start, which are [[gridspec-inventory-mutations]]'s to measure,
  and does not clear a prim's "inventory is empty" flag. Not measured: a
  move of an item the agent may not copy and a drop of one; a script under
  an experience; who is told when a notecard or a script in a prim is
  saved over ([[test-asset-save-mutation-survey]]); a prim in a
  neighbouring region; a friend with edit rights or a group member in the
  stranger's place; the serial at its sixteen-bit wrap.

## Known already

`RezScript` works on both grids (the long-standing "SL drops it" was the
harness scripting a stranger's prim, [[test-phase-z-deferred-04]]); SL leaves
`group_owned` out of a contents listing unless it is true and needs the
listing's `Xfer` to name remote path `LL_PATH_CACHE`; current viewers fetch the
listing over the `RequestTaskInventory` capability instead
([[protocol-request-task-inventory-cap]]). OpenSim mints a fresh item id on
copy-in. The fake grid ignores `RezScript`,
`MoveTaskInventory`, `RemoveTaskInventory`.

The serial-push half of an `UpdateTaskInventory` drop is measured
([[gridspec-object-properties]], `book/src/gridspec/objects.md` § Properties,
2026-10-09): Second Life sends the record with its advanced serial to every
session holding the prim selected, the writer only if it is one; OpenSim to
the writer alone, selected or not. The drop itself lands on Second Life. Left
for this task: the same question for `RezScript`, a remove and a move.

## Discover

Run `task-inventory` on both grids; add remove / move-to-agent legs; the
`UpdateTaskInventory` drop on SL; the serial-push half
with a second avatar.

## Document

`book/src/gridspec/building.md` § Contents.

## Fake grid

Small in this task: `RezScript`, remove, move; an SL drop imitation once the
cause is known.

## Viewer

The Contents tab survives a write that does not land.

## Capabilities done in this task

[[protocol-request-task-inventory-cap]] (task inventory over
`RequestTaskInventory`) and [[protocol-cap-get-metadata]] (a script's
experience over `GetMetadata`). Also the protocol halves of
`UpdateGestureTaskInventory` (both grids; the editor is
[[viewer-gesture-management-ui]]) and `UpdateMaterialTaskInventory` (Second
Life; the editor half is [[viewer-material-save-to-object]]).
