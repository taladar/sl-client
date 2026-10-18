---
id: gridspec-terrain-editing
title: Terraforming, raw terrain transfer and terrain textures on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-region-options-terrain, gridspec-aditi-test-land]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim `UndoLand` is a stub, raise / lower limits ±100 m (SL 4 m); the fake
grid handles no `ModifyLand`.

## Discover

`modify-land`, `terrain-raw-*` on OpenSim; aditi on a sandbox that allows
terraforming.

## Document

`book/src/gridspec/terrain.md` § Editing.

## Fake grid

Large — [[server-fake-grid-terraform]].

## Viewer

Per-grid limits from `RegionInfo`; no reliance on undo on OpenSim.

## Capabilities done in this task

The protocol half of `ModifyRegion` (Second Life): reading and writing the
region's PBR terrain overrides; rendering them is [[viewer-pbr-terrain]].

## Done (2026-10-18)

The `terrain-editing` census (2av, all four grids) measures each brush in
point and area form, undo, the limits for the owner and for a manager, bake,
the RAW transfers, the terrain tab and `ModifyRegion`, for an avatar with
rights and one without (`book/src/gridspec/terrain.md` § Editing). Findings:

- Neither grid tells a refused stroke, bake, texture write or RAW transfer
  with anything at all; on aditi neither test avatar may terraform.
- OpenSim: a stroke sends the one patch it moved to everybody; undo does
  nothing; the region owner is a god by default and so not held to the
  limits, a manager is (counted from the baked map); `RevertSphere` reaches
  only `√radius`; a terrain-tab save tells five `RegionInfo`s and four
  handshakes; the RAW upload's first alert comes before the last confirm; a
  RAW load re-bakes; an unchanged upload re-sends nothing.
- Second Life grants `ModifyRegion` once the seed asks for it, answers four
  `undef` overrides, and refuses a resident's write `403` with an LLSD reason.

Client: `CAP_MODIFY_REGION` is requested;
`Command::RequestTerrainMaterialOverrides` / `SetTerrainMaterialOverrides` and
`Event::TerrainMaterialOverrides` /
`TerrainMaterialOverridesWritten` in both runtimes, a refusal carrying the
grid's reason. Fake grid: `ModifyRegion` served on `FakeSl` with Second Life's
answers; RAW transfers and bake a manager's only; `FakeOpensim` narrates the
transfers in OpenSim's alerts (the upload's ahead of the last confirm, via
`SimSession::request_xfer_upload_announced`) and tells terrain heights per
corner. Viewer: a brush in a region that blocks terraforming is refused with
`RegionNoTerraforming` before anything is sent. Terraforming in the fake grid
is [[server-fake-grid-terraform]] (its census rows are exempt on `FakeOpensim`
until then); the parcel-level refusal is [[viewer-terraform-parcel-refusal]].
