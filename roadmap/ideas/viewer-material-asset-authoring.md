---
id: viewer-material-asset-authoring
title: Create and upload GLTF material assets
topic: viewer
status: ideas
origin: audit of menu entries still gated on UNIMPLEMENTED (2026-08-24)
refs: [viewer-image-upload, viewer-pbr-material-editor]
---

Context: [context/viewer.md](../context/viewer.md).

Three inventory entries still greyed, all about a GLTF material as an
*asset* rather than as something already on a face: **New Material** (create
a blank material asset in a folder), **Upload ▸ Material...** (import a
`.gltf` / `.glb` material), and saving an edited material back as a new
inventory item.

The viewer can already edit a material on a prim face and knows the asset
format. What is missing is the inventory-side half: creating the asset,
naming and filing it, the upload path and its cost, and the round trip back
out of the material editor into a new item.
