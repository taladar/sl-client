---
id: server-fake-grid-material-edits
title: Fake grid — apply legacy and PBR material edits and push overrides
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-materials]
---

Context: [context/server.md](../context/server.md).

## What

Material edits are acked and dropped. Apply them to the texture entry, push GLTF
overrides on the SL flavour, refuse PBR on the OpenSim flavour.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Measured (2026-10-12, [[gridspec-materials]])

`book/src/gridspec/materials.md` is the model. What it asks of the fake grid:

- **Legacy, both flavours.** A PUT writes the id into the face's texture
  entry of the *region's* object and sends everybody a full update (Second
  Life: a terse one where a face that had a material gets another). A set
  that changes nothing tells nobody. A stranger's set is answered `200` with
  an empty array and changes nothing. A material id in an `ObjectImage` is
  ignored.
- **Per flavour** (a row each in `imitates.rs`): the PUT's answer (an entry
  per face taken / always an empty array); the `GET` of every material (all
  of them / always empty); the viewer's default material (kept / no
  material); a face the prim has not got (nobody told / a full update all
  the same); a PUT of no face (`400` / an empty array); a body that does not
  read (`400` / `200` with no body); and the id — the two grids hash the same
  material differently. OpenSim's is `FaceMaterial.genID`; Second Life's is
  not known, so the Second-Life flavour's ids can be its own as long as the
  same material gets the same id.
- **PBR, Second-Life flavour only.** The asset into the extra parameters
  and a full update; overrides kept **per face**, independent of the
  material, replaced whole by each update, cleared by an empty `gltf_json`
  or a GLTF material that overrides nothing; after every change one
  `GenericStreamingMessage` naming every face that has one (an empty `te`
  when none is left), to the editor too; the same message after a full
  update sent on `RequestMultipleObjects`. Refusals: `403 permission denied`
  for a stranger, `404 Invalid side: N`, `404 failed to find task`, `400`
  for a body with no `object_id` and for JSON that does not parse. The
  override's notation keys are in the chapter (`bc`, `ec`, `mf`, `rf`, `am`,
  `ac`, `ds`, `tex`, `ti`); turning `gltf_json` into them is the encoder
  half `sl-material` does not have yet.
- Then add `Grid::FakeSl` and `Grid::FakeOpensim` to the `materials` case.
