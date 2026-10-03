---
id: gridspec-materials
title: Legacy and PBR material edits and overrides on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-pbr-material-editor]
---

Context: [context/gridspec.md](../context/gridspec.md).

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
