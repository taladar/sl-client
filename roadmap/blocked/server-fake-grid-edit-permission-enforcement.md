---
id: server-fake-grid-edit-permission-enforcement
title: Fake grid — refuse edits the editor may not make, as each grid refuses them
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-object-edit]
---

Context: [context/server.md](../context/server.md).

## What

Any session can edit any prim today. Enforce modify / move / copy rights with
each grid's refusal shape (silence, corrective update, alert).

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
