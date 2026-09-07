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
**Half of this is now measured** (`object-asset-format`, 2026-09-06): a viewer
cannot fetch an object asset on Second Life *at all*. Every object item the
test account holds answers with a nil `asset_id`, in the AIS3 listing and the
per-item fetch alike, including items that are full-perm to their owner. So the
first possibility below cannot be settled from a viewer, and the missing-field
question only ever had an answer on OpenSim — where the body is
`SceneObjectSerializer` XML and these keywords do not exist either.

What is left of this task is therefore narrower than it was written. Half of
what was left has since been settled by
[[test-fake-grid-object-asset-id-divergence]] (2026-09-06): the fake grid
**stopped** serving a taken object's asset by default — `ObjectAssetPolicy`'s
default is `Withheld`, Second Life's side, where the item carries a nil asset
id and no capability reaches the body. So on the default configuration the
missing keywords are unobservable to a viewer, and the question narrows again:
whether the prims written under `ObjectAssetPolicy::Served` — OpenSim's side,
which `asset-round-trip` asks for — should carry the fields the 2005 captures
lack.

That is now a smaller and stranger question than it looks, because the grid
`Served` imitates does not write this format at all: OpenSim writes
`SceneObjectSerializer` XML, and the fake grid writes the Linden text form
under the same policy name. Whether that pairing is worth keeping — or whether
`Served` should grow an XML body to match the grid it names — is the decision
this task now carries, alongside the original missing-keyword list.

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
