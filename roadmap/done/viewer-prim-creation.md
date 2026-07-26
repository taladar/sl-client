---
id: viewer-prim-creation
title: Prim / Linden tree / grass creation (the Create tool)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); tree/grass modes added on user request (2026-07-23)
blocked_by: [viewer-object-edit-floater-shell]
---

Context: [context/viewer.md](../context/viewer.md).

Create new in-world objects: a **Create** tool mode in the Build Tools
floater ([[viewer-object-edit-floater-shell]] — a fourth tool button beside
Move / Rotate / Stretch), pick a base type, and rez it at a ray-cast build
point on a surface, then drop into edit on the new object (the reference
keeps the placer active for repeat-rez with a held modifier). This is the
entry point to the build workflow.

The type picker covers all three of the reference's create families:

- the **prim** volume types (box, cylinder, prism, sphere, torus, tube,
  ring — the reference's per-type buttons);
- **Linden trees** (`pcode` TREE / NEW_TREE with the species byte in
  `state` — remember OpenSim's `AdaptTree` ×8 scale quirk from the
  `rez_sample_trees` example);
- **Linden grass** (`pcode` GRASS with the species byte).

All three rez through the same `ObjectAdd` message, differing only in
`pcode` / `state`; the `rez_sample_prims` / `rez_sample_trees` /
`rez_sample_grass` examples in `sl-client-tokio` already exercise the wire
side of each and are the reference for the parameters.

Reference (Firestorm, read-only): `lltoolplacer` (incl. its tree / grass
placer variants), `lltoolcomp` (create); the `ObjectAdd` message.

Builds on: `objects.rs` lifecycle, `sl-prim` tessellation, `sl-tree`
tree / grass geometry, and [[viewer-default-creation-permissions]] for the
rezzed prim's default perms.

## Done

`sl-client-bevy-viewer/src/edit_create.rs`. **Create** is the first Build Tools
tool-mode button ([`EditTool::Create`], before Move / Rotate / Stretch / Select
Face). While it is active the per-aspect tabs are replaced by a
**create panel**: a base-type radio of the seven prim volume types plus **Tree**
and **Grass**, and — for a plant base — a species combo (the full `trees.xml` /
`grass.xml` tables). A left click on a world surface ray-casts the build point
and rezzes the picked type there via `Command::RezObject` (the shared
`ObjectAdd`), the three families differing only in `pcode` / `state`; a click on
an avatar / attachment is refused. The prim volume params (path / profile /
top-size / shear bytes and the sphere / torus-family 90° upright rotation) are
the reference `LLToolPlacer::addObject` values, not the looser
`rez_sample_prims` approximations, so the tube/torus/ring/prism render
correctly. After the rez the tool **drops into edit** on the new object —
resolved by polling the tracked scene for a matching root (robust to spawn
timing), then selected and switched to the Move tool — unless `Shift` is held,
which keeps the placer active for repeat-rez. The Create tool shows a
procedurally-drawn **magic-wand cursor** while the pointer is over the world. A
`build-create` gallery specimen covers the panel shape in the `ui_test` matrix.

Deliberately out of scope / follow-ups: Shift-drag duplication of a selection
([[viewer-create-shift-drag-duplicate]]); the rezzed prim's default permissions
([[viewer-default-creation-permissions]]). Note: object-pie **Delete** on
OpenSim is blocked by an unrelated Trash-folder gap
([[viewer-opensim-trash-folder-not-resolved]]), not this task.
