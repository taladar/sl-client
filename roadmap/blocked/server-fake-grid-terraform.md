---
id: server-fake-grid-terraform
title: Fake grid — terraforming, undo and bake
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-terrain-editing]
---

Context: [context/server.md](../context/server.md).

## What

`ModifyLand` / `UndoLand` are unhandled and a raw upload re-broadcasts nothing.
Brushes, patch re-broadcast, undo honoured per flavour, bake baseline.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
