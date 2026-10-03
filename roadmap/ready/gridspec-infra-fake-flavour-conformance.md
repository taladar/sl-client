---
id: gridspec-infra-fake-flavour-conformance
title: Run every conformance case against both fake-grid flavours, beside the live grids
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

`sl_conformance::fake::run_offline_case` runs a listed case against the fake
grid and fails one that records `partial`. Only a handful of cases declare
`Grid::FakeSl`, almost none `FakeOpensim`, and nothing compares a fake run
with what the same case recorded on the live grid it imitates.

## Discover

Nothing to measure on a live grid; this is the harness that lets every other
`gridspec-*` task prove its fake-grid half. Decide how a case states the
per-grid expectation it learned (constants in the case, keyed by `Grid`,
citing the book table) so that a `FakeSl` run is held to the aditi answer
and a `FakeOpensim` run to the OpenSim one.

## Document

`book/src/conformance/` — how a case pairs a live grid with the fake flavour
imitating it, and the "a gridspec feature is done when" checklist from
`roadmap/context/gridspec.md`.

## Fake grid

Make the offline runner take either flavour; make `sl-conformance-report`
show live and fake results of a case side by side. Each later `gridspec-*`
task adds its cases to the offline list for both flavours.

## Viewer

Not applicable.
