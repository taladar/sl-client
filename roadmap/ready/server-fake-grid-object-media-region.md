---
id: server-fake-grid-object-media-region
title: Fake grid — region-wide object media with version bumps and propagation
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-object-media]
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

## Measured (2026-10-12)

[[gridspec-object-media]] measured it; `book/src/gridspec/media.md` § Media on
a prim is the model, and the case `object-media` holds a run to 187 answers.
To do here:

- media as region state on the object, a version `x-mv:<10 digits>/<agent>`
  naming the last sender, in the object's `MediaURL` and sent to every
  session in a full update;
- the face's media flag set and cleared as each grid does, and a `GET` that
  states no media for a face without it;
- each flavour's refusals: Second Life's `200` with an `error` map and its
  `404`s, OpenSim's `400`s, the permission and whitelist rows, and
  OpenSim's quirks (a stranger's `UPDATE` versioned and announced, a short
  `UPDATE` half applied) as rows in `imitates.rs`;
- add `FakeSl` and `FakeOpensim` to the case's grids and to
  `sl-conformance/tests/offline.rs`;
- an `e2e` test on each flavour that the viewer refetches when another
  session navigates.
