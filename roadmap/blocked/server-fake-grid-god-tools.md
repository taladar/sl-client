---
id: server-fake-grid-god-tools
title: Fake grid — god tools as OpenSim answers them
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-god-tools]
---

Context: [context/server.md](../context/server.md).

## What

Every god `ServerEvent` is unmatched. Implement what OpenSim does for a
god-level account; low priority.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
