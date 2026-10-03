---
id: server-fake-grid-object-media-region
title: Fake grid — region-wide object media with version bumps and propagation
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-object-media]
---

Context: [context/server.md](../context/server.md).

## What

ObjectMedia lives per session today; make it region state, bump `MediaURL`,
re-stream to others, refuse per permission.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
