---
id: test-object-asset-missing-fields
title: A take through the object asset drops half a prim
topic: test
status: ready
origin: doing test-assets-object-asset-codec (2026-09-06)
points: 2
refs: [test-assets-object-asset-codec, test-fake-grid-object-write-path]
---

Context: [context/testing.md](../context/testing.md).

The object asset text has no keyword for several things a live prim carries,
so a take followed by a rez loses them. [[test-assets-object-asset-codec]]
established the list while reconstructing the format, and this is what to do
about it:

- a face's `glow` and its legacy material id (the text's `faces` block ends at
  `media_flags`);
- the whole `ExtraParams` block — flexi, light, sculpt, **mesh**, light image,
  extended mesh, render material, reflection probe. A mesh object taken and
  rezzed through this format comes back as its shape block, which is a box;
- floating text (`llSetText`), though its *colour* is written (`textcolor`);
- a media URL, a texture animation, a particle system.

Two possibilities, and the work is deciding which:

- **The reference writes fields nobody here has seen.** Both reference assets
  are from 2005, before flexi prims, sculpties, mesh and materials existed, so
  the absence may be an artefact of their age rather than of the format. A
  modern take from Second Life would settle it — one object with a light, a
  flexi path and a glowing face, taken and its asset fetched. That is a live
  task, and it is the same fetch [[test-asset-save-mutation-survey]] does.
- **The format really is that old**, and a modern grid stores something else
  entirely (an LLSD or XML serialisation) for anything the text cannot say.
  OpenSim is the settled half of that already: verified while doing
  [[test-assets-object-asset-codec]], it writes
  `SceneObjectSerializer.ToOriginalXmlFormat` — or
  `CoalescedSceneObjectsSerializer.ToXml` for a multi-object take — as the
  `AssetType.Object` body (`InventoryAccessModule.cs:527-587`), and carries not
  one keyword of this text format anywhere. So the two grids already disagree
  about what `AssetType::Object` *is*; what is unmeasured is only the Second
  Life side.

Until then `sl-object-asset` says what it cannot carry and the fake grid takes
only the prims it rezzes (a box), so nothing silently loses a field it was
given.

Acceptance: a record of what a live grid's object asset actually contains for a
prim with a light, a flexi path, a sculpt/mesh and a glowing face — and either
the missing keywords added to the codec, or a written statement that the class
is two different formats on the two grids.
