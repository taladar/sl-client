---
id: server-fake-grid-inventory-offers
title: Fake grid — inventory offers, filing and object gives
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-inventory-offers, server-fake-grid-im-relay]
---

Context: [context/server.md](../context/server.md).

## What

File offered items at offer time (OpenSim) or on accept (SL), relay or withhold
the giver ack, deliver object gives.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
