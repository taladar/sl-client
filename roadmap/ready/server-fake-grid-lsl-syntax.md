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

## Folded in from protocol-sim-lsl-syntax-document (2026-10-15)

[[protocol-sim-lsl-syntax-document]] asked for the same thing from the LSL
side and is closed as a duplicate of this item. What it adds:

- **Which document, decided here.** It wanted the document *rendered from
  the engine's own descriptor table* ([[server-lsl-library-surface-table]],
  done) rather than from a copy of Linden's list, so the editor colours and
  completes exactly the functions this grid can run — more honest about the
  fake grid, and a test of the table → document → `sl-wire` decoder round
  trip. The vendored-document route above imitates the live grids instead.
  Pick one per flavour and say why (or offer both behind a knob);
  [[server-lsl-lib-ossl]] makes the same point for the `os*` functions.
- **The id follows the content**, so a changed table changes the
  advertised `LSLSyntaxId` and invalidates a viewer's cached document.
- **Fixture knobs for the two cases a viewer must survive**: a grid that
  advertises no id (serve nothing; the viewer falls back to its shipped
  default) and one serving a document at an unsupported version, which the
  decoder refuses (`WireError::UnsupportedLslSyntaxVersion`).
- **A round-trip unit test**: document → `sl-wire`'s decoder → `LslSyntax`
  equals its source.

Acceptance also includes: a viewer logged into the fake grid fetches a
non-empty `LSLSyntax` document and its editor highlights and completes from
it.

## Measured (2026-10-26, [[gridspec-lsl-ossl]])

OpenSim's served document is not a list of what the region allows: it
carries every `os*` function whatever the `[OSSL]` configuration denies or
disables, and it leaves out `osListenRegex` and every `mod*` function, which
compile and run there. A `FakeOpensim` document vendored from it imitates
those gaps too; one rendered from the engine's table would not. Say which
the flavour is meant to imitate.
