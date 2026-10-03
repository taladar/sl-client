---
id: server-fake-grid-mute-list
title: Fake grid — mute list storage, the Xfer file and each grid's empty-list reply
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-mute-list]
---

Context: [context/server.md](../context/server.md).

## What

Mute requests are dropped. Decode them server-side, store, serve the CRC-cached
file and `emptymutelist`, enforce where the grid enforces.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
