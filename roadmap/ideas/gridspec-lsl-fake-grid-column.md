---
id: gridspec-lsl-fake-grid-column
title: Fill the LSL gridspec's fake grid column from the probe results
topic: gridspec
status: ideas
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [test-lsl-differential-corpus-triage]
---

Context: [context/lsl.md](../context/lsl.md).

`book/src/gridspec/lsl.md` still says "no scripts" in its fake grid
column in about 500 rows, and its introduction says the fake grid does not
run the library yet. Both are out of date: the fake grid runs the library
on both flavours, and `probes/results/fake-sl/` and
`probes/results/fake-opensim/` record what each flavour printed for every
probe, beside the `accepted.toml` entries that name where it still differs.

Wanted: each row's fake grid cell says what the fake grid does, read from
those results: "as Second Life" or "as OpenSim" (per flavour) where they
agree, or the difference and its accepted entry or task where they do not.
Rewrite the introduction to match. Generate the column from the records
where a row maps to a probe record, as the gridspec census workflow does,
rather than writing 500 cells by hand.

Acceptance: no "no scripts" cell is left for behaviour a probe measures,
and every remaining difference names its `accepted.toml` entry or task.
