---
id: server-fake-grid-parcel-access-enforcement
title: Fake grid — enforce parcel access and push ban lines
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-parcel-access-and-ban-lines,
  server-fake-grid-parcel-on-movement, server-world-agent-movement]
---

Context: [context/server.md](../context/server.md).

## What

A banned agent walks in today. Enforce access, push ban lines with the negative
sequence ids, eject as each grid ejects.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
