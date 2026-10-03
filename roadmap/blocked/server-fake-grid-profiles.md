---
id: server-fake-grid-profiles
title: Fake grid — profiles, picks, classifieds and notes with each grid's quirks
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-profiles, protocol-sim-profile-messages]
---

Context: [context/server.md](../context/server.md).

## What

Store and serve profiles; OpenSim's ignored queries and volunteered replies as
flavour rows.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
