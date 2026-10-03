---
id: server-fake-grid-material-edits
title: Fake grid — apply legacy and PBR material edits and push overrides
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-materials]
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
