---
id: server-fake-grid-mesh-upload-cost
title: Fake grid — mesh upload costing and refusals
topic: server
status: blocked
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
