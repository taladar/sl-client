---
id: server-fake-grid-directory-search
title: Fake grid — answer directory searches from an index of accounts, parcels and fixtures
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-search-directory]
---

Context: [context/server.md](../context/server.md).

## What

The protocol exists but the fake grid answers no `Dir*` query. Build the index;
OpenSim's people / groups-only answers and SL's sentinel as flavour rows.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
