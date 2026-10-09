---
id: server-fake-grid-object-record-texture-ids
title: Fake grid — list face textures in a Second Life object record
topic: server
status: ready
origin: gridspec-object-properties (2026-10-09)
refs: [gridspec-object-properties, server-fake-grid-object-update-forms]
---

Context: [context/server.md](../context/server.md).

Second Life's `ObjectProperties` carries a `TextureID` list: one id for each
face of the prim, not deduplicated — a new cube's reads the default texture
six times (`object-properties`, aditi, 2026-10-09,
`book/src/gridspec/objects.md` § Properties). OpenSim sends none
(`CreateObjectPropertiesBlock`: "still not sending, not clear the impact on
viewers"). The fake grid sends none on either flavour, and the case's
`texture_ids` metric is the one field of a new prim's record no run is held
to.

It was left out of [[gridspec-object-properties]] because the list needs the
prim's **face count**, and the fake grid has none: an `Object` holds its
texture entry as the undecoded blob, and `decode_texture_entry` takes the
count from its caller. The count follows from the shape (path and profile
curves, cut, hollow, and for a sculpt or a mesh something else again), which
the viewer's prim mesher works out and nothing below it does.

## To do

- A face count for a prim's shape somewhere the fake grid can reach — in the
  pure crate that owns the shape, not copied out of the viewer's mesher.
- `PropertiesPolicy::lists_textures` (`sl-fake-grid/src/imitates.rs`): on
  for Second Life, off for OpenSim; `SceneFixtures::record_of` fills
  `texture_ids` from the decoded texture entry, one id a face.
- Measure before building what is not known yet: what the list holds for a
  **child** prim's record and for a root's (the reference viewer reads it as
  the linkset's), and for a sculpt and a mesh. `object-properties` read a
  lone cube only.
- `texture_ids` joins the case's table of measured answers, so both live
  grids and both flavours are held to it.

No viewer code of ours reads the list today.
