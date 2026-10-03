---
id: server-fake-grid-inventory-udp-mutations
title: Fake grid — the UDP inventory mutations, Trash gating and pushes
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-inventory-mutations]
---

Context: [context/server.md](../context/server.md).

## What

About eight UDP inventory mutation messages are dropped. Decode and apply them,
gate purge / remove on Trash, push what each grid pushes, refuse what SL
refuses.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
