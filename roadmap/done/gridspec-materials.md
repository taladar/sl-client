---
id: gridspec-materials
title: Legacy and PBR material edits and overrides on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-pbr-material-editor,
  server-fake-grid-material-edits, gridspec-lsl-prim-state]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-12)

Measured and written up in `book/src/gridspec/materials.md`.

- **Discover.** A new case, `materials`: one avatar rezzes a cube, a second
  stands by, and the case sends each request to the capability itself (status
  and body), then listens on both for the cube sent again and for override
  messages. `RenderMaterials`: the reads a region cannot serve; a set, its
  id, the read back, the same set again, the same material on another face,
  another material, the viewer's default material, two faces at once, a face
  and a prim that are not there, the other avatar's set and clear, a clear,
  and an `ObjectImage` that leaves a material id out or states one.
  `ModifyMaterialParams`: the requests a region cannot serve; an override
  with no material under it, the material, an override, the same, another,
  both at once, text that is no GLTF, an override of nothing, an asset that
  does not exist, the other avatar's, a legacy material on the same face, the
  override cleared, the material replaced and cleared, every face at once,
  and the object asked for again. The client's own commands. Four runs on
  aditi (Mauve) and four on OpenSim, held to 232 measured answers.
- **Findings.** OpenSim grants no `ModifyMaterialParams` (nor the two
  material-saving capabilities): legacy only. A legacy id is a hash both
  grids make of the material and write into the texture entry, but **not the
  same hash**; neither takes an id from an `ObjectImage`. OpenSim answers a
  PUT with an empty array always, Second Life with an entry per face it
  took; OpenSim's `GET` of every material is always empty; OpenSim does not
  keep the viewer's default material. Both refuse a stranger's set silently.
  Second Life refuses a PBR edit with an HTTP status and `success` false
  (`403 permission denied`, `404 Invalid side`). A PBR asset is named in the
  extra parameters and told by a full update; an override by a
  `GenericStreamingMessage` about 0.2 s later, to the editor too, **naming
  every face of the object that has one**. An override belongs to the face:
  kept with no material under it, replaced whole by the next, surviving the
  material's replacement and clearing, and sent after a full update to
  whoever asks for the object again. An asset id that does not exist is
  taken.
- **Fake grid.** Not here, as planned: [[server-fake-grid-material-edits]]
  is now unblocked and has the measured model; the case lists the live grids
  only until it lands.
- **Viewer.** The build floater's PBR tab is greyed on a region that grants
  no `ModifyMaterialParams` (unit tests, and an `e2e` test on both fake
  flavours). Override handling already matched what was measured; an
  override the viewer cannot read is now logged rather than taken for a
  clear. `sl-client-tokio` did not re-export `FaceMaterialPut`, which
  `Command::SetRenderMaterials` takes; it does now.
- **Not measured.** Child prims, attachments, meshes; take, rez, copy and
  link; the group bit and edit rights; a no-modify prim; an asset the agent
  has no right to; the per-transaction limit; how Second Life hashes a
  material; and what a script sets ([[gridspec-lsl-prim-state]]).

## Known already

OpenSim has legacy `RenderMaterials` only; `ModifyMaterialParams` / GLTF
overrides are SL-only; the fake grid acks and drops material edits.

## Discover

A materials case: legacy PUT, PBR `ModifyMaterialParams`, override delivery and
timing, echo to the editor, OpenSim's answer to a PBR edit.

## Document

`book/src/gridspec/materials.md`.

## Fake grid

Large — [[server-fake-grid-material-edits]].

## Viewer

PBR apply hidden / disabled on OpenSim.
