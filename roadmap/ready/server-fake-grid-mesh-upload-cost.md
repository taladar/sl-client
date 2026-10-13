---
id: server-fake-grid-mesh-upload-cost
title: Fake grid — mesh upload costing and refusals
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-asset-upload]
---

Context: [context/server.md](../context/server.md).

## What

Answer step 1 of a mesh upload with each grid's cost data and refuse what each
refuses.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Measured (2026-10-13)

[[gridspec-asset-upload]] is done: both grids' first answers for a unit cube
(the price, the breakdown, the cost data) and their refusals of a model with
no physics shape, of noise and of empty lists are in
`book/src/gridspec/assets.md` § A mesh, and the case `upload-http` records
them (`step1_mesh*`). The fake grid answers the price alone today; those
keys are the ones a fake run is not held to (`NOT_IMITATED` in
`sl-conformance/src/cases/upload_http.rs`) — take them off that list as they
are imitated. The upload itself is not measured on Second Life: the test
accounts' `mesh_upload_status` is `invalid`.
