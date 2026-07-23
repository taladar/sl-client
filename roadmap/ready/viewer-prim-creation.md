---
id: viewer-prim-creation
title: Prim / Linden tree / grass creation (the Create tool)
topic: viewer
status: ready
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
