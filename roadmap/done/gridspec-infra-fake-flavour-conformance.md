---
id: gridspec-infra-fake-flavour-conformance
title: Run every conformance case against both fake-grid flavours, beside the live grids
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-04)

- **Every offline case runs on both flavours.** The 28 offline cases that
  declared only `FakeSl` now declare `FakeOpensim` too, and all 36 pass on both
  as they stand: nothing in them depended on the Second Life flavour. The one
  exception, `asset-round-trip` (only OpenSim lets a viewer read a taken
  object's asset back), is listed in `fake::SINGLE_FLAVOUR` with that reason; a
  unit test holds every other offline case to both.
- **One test per case and flavour.** `tests/offline.rs` declares
  `<case>::fake_sl` / `<case>::fake_opensim`, so a failure names the grid and
  the two run in parallel (the suite went from 93 s to 51 s on a release
  build). `fake::run_offline_case(test, flavour)` refuses a flavour the case
  does not declare.
- **Per-grid expectations: `measured::Measured<T>`** — the Second Life and
  OpenSim answers plus a `source` naming the book table, case and date.
  `Measured::check(what, grid, &actual)` picks the answer by
  `Grid::behaves_like`, so `FakeSl` is held to aditi's answer, `FakeOpensim` to
  OpenSim's, and each live grid to its own (a live mismatch means the grid or
  the table moved). `simulator-features` and `object-asset-format` were
  converted to it.
- **Side by side in the report.** `Grid::REPORTED` pairs each live grid with
  its twin (`opensim`, `fake-opensim`, `aditi`, `fake-sl`), and under each case
  the reporter lists every recorded field the fake twin answers differently
  (`report::divergences`, skipping timings and ids on both sides).
  `sl-conformance run-offline [--grid …]` records every offline case on the
  fake grid in one go, through the same `fake::run_case` the test suite uses,
  so those columns have data.
- **Book:** `book/src/conformance/runner.md` gained *Holding a flavour to its
  live grid* (the `Measured` pattern and the four-part "a gridspec feature is
  done when" checklist) and *Live and fake side by side*; `overview.md` and
  `records.md` follow.

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
