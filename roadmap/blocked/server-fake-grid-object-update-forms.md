---
id: server-fake-grid-object-update-forms
title: Fake grid — send compressed, terse and cached object updates as each grid does
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-object-update-stream]
---

Context: [context/server.md](../context/server.md).

## What

Today every object goes out as a full `ObjectUpdate` at arrival. Send the forms
each grid sends (compressed, terse, cache probes honouring the handshake flags).

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
