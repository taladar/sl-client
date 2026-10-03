---
id: server-fake-grid-estate-actions
title: Fake grid — kick, eject, freeze, teleport home, estate messages and restarts
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-estate]
---

Context: [context/server.md](../context/server.md).

## What

None of the estate actions is answered. Remove agents, teleport them home,
deliver estate messages and restart notices per flavour.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
