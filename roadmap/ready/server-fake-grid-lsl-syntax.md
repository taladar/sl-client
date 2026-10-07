---
id: server-fake-grid-lsl-syntax
title: Fake grid — serve each flavour's LSL syntax document and advertise its id
topic: server
status: ready
origin: gridspec-simulator-features (2026-10-07)
refs: [gridspec-simulator-features, gridspec-lsl-compile, gridspec-lsl-ossl]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Observation

Both live grids advertise an `LSLSyntaxId` in `SimulatorFeatures` and serve
the document it names from the `LSLSyntax` capability: Second Life one id
per simulator version, with an `LSLSyntaxVersion` beside it (`0.6.12` and
`0.6.13` on two aditi regions, 2026-10-07); OpenSim the id on the first line
of `bin/ScriptSyntax.xml`, whose document adds the `os*` functions.

The fake grid advertises neither key on either flavour, because it serves no
syntax document: `SimSession::set_lsl_syntax` is never called by
`sl-fake-grid`, so the capability would answer an empty table. A viewer on
the fake grid therefore never takes the fetch-and-cache path it takes on
every live login, and `simulator-features` exempts the two keys for the fake
flavours (`FAKE_GRID_OMITS`).

## Work

- Give each flavour a syntax document: Linden's, already vendored as
  `sl-lsl-runtime/keywords_lsl_default.xml`, for `FakeSl`; one carrying the
  `os*` functions for `FakeOpensim` (OpenSim's `ScriptSyntax.xml` is BSD —
  decide between vendoring it and deriving the additions).
- Seed it on every session with a stable id per flavour, and add
  `LSLSyntaxVersion` to the Second Life flavour's stock features.
- Drop `FAKE_GRID_OMITS` from `sl-conformance/src/cases/simulator_features.rs`
  so both flavours are held to the whole table.
- Check the cost: the document is 683 KB and every fake-grid login in the
  test suite would fetch and parse it once.
