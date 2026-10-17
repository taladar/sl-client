---
id: server-fake-grid-parcel-divide-join
title: Fake grid — divide and join parcels
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
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

## Measured (2026-10-05, [[gridspec-parcel-management]])

OpenSim answers neither message: it re-sends the overlay, nothing else.
Second Life is unmeasured. `book/src/gridspec/land.md` § Dividing and joining.

## From gridspec-land-transactions (2026-10-17)

`land-transactions` divides its plot off the region on OpenSim, so it cannot
run against either fake flavour until this lands: add it to the offline list
for both (`sl-conformance/src/fake.rs`) here, with the fake grid's land rules
(`sl-fake-grid/src/land_sales.rs`) already in place.
