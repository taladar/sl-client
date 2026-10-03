---
id: server-fake-grid-parcel-divide-join
title: Fake grid — divide and join parcels
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-parcel-management]
---

Context: [context/server.md](../context/server.md).

## What

`DivideParcel` / `JoinParcels` are unhandled. Bitmap surgery, local id
allocation, overlay re-send, pushes to occupants.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
