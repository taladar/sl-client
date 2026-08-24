---
id: viewer-upload-model
title: Model / mesh upload wizard
topic: viewer
status: ideas
origin: audit of menu entries still gated on UNIMPLEMENTED (2026-08-24)
refs: [viewer-image-upload, viewer-mesh-lod-decimation]
---

Context: [context/viewer.md](../context/viewer.md).

The inventory's **Upload ▸ Model...** entry, greyed since the menu was
written. [[viewer-image-upload]] covers the simple uploads — texture, sound,
animation, bulk — and explicitly stops short of this one, because a mesh
upload is not a file picker with a cost label: it is the LOD/physics wizard.

What the reference asks for before it will send anything: the four LOD slots
(generate or supply each), a physics shape (from a LOD, a supplied mesh, or a
hull decomposition), skin weights and joint positions for a rigged mesh, the
per-LOD triangle/vertex accounting the land-impact charge is computed from,
and the upload-cost breakdown that follows from all of it. The wire side is
`NewFileAgentInventory` plus the mesh asset format, both of which the
workspace already speaks.

Worth doing after [[viewer-mesh-lod-decimation]], which owns the decimation
this wizard would drive for generated LODs.
