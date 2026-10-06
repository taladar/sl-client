---
id: server-fake-grid-parcel-access-enforcement
title: Fake grid — enforce parcel access and push ban lines
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-sl-ban-line-trigger]
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

## Measured (2026-10-05)

`book/src/gridspec/land.md` § At the parcel's edge and § The ban line, by
`parcel-ban-enforcement` (OpenSim) and `parcel-ban-line` (aditi). The OpenSim
flavour is fully specified there: who is exempt, the two alert texts, the
push under `-30000` / `-40000` on movement near the parcel, the avatar put
back outside, a teleport that lands and is then moved, a ban that bites only
once the avatar moves. The Second Life flavour has the refusal
(`NOTIFY: Cannot enter parcel: …`), the walking avatar stopped at the line and
the flying one lifted over it; when it pushes the line is
[[gridspec-sl-ban-line-trigger]], and a ban proper waits on land.
`parcel-ban-enforcement` is the case to add to the offline list once this is
built.
