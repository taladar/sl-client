---
id: protocol-sim-lsl-syntax-document
title: The fake grid serves an empty LSLSyntax document
topic: protocol
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 3
blocked_by: [server-lsl-library-surface-table]
refs: [viewer-lsl-editor-highlight, protocol-lsl-syntax]
---

Context: [context/lsl.md](../context/lsl.md).

`SimCaps` serves the `LSLSyntax` capability from
`build_lsl_syntax_document(sim.lsl_syntax())`, and `sl-fake-grid` never
calls `set_lsl_syntax`. So the document the fake grid serves is empty:
no functions, no constants, no events.

`sl-wire`'s decoder is explicitly tolerant of that — "an empty top-level
document is not treated as a decode error … so the caller decides
whether an empty table means use a shipped default" — so nothing breaks.
But everything downstream of the table is inert against this grid: the
editor's highlighting ([[viewer-lsl-editor-highlight]]), autocomplete,
tooltips, the semantic pass's undefined-symbol check, and `sl-lsl-lsp`
against a fake-grid session all have nothing to look up. The whole
design of keeping the library **out** of the lexer and taking it from
the grid at runtime is untestable offline.

Wanted:

- `sl-fake-grid` setting a real syntax document, **rendered from the
  engine's own descriptor table** ([[server-lsl-library-surface-table]])
  rather than from a copy of Linden's list — so the editor colours and
  completes exactly the functions this grid can run, which is both more
  honest and a better test of the round trip;
- `SimulatorFeatures` advertising the matching `LSLSyntaxId`, which is
  what makes a viewer fetch the document at all, with the id derived
  from the table's content so it changes when the table does (and a
  cached document is correctly invalidated);
- a fixture knob for the other two cases a viewer must survive: a grid
  that advertises no id (serve nothing, viewer falls back to its shipped
  default) and one that serves a document at an **unsupported version**,
  which the decoder is version-gated to refuse
  (`WireError::UnsupportedLslSyntaxVersion`);
- a round-trip test: the table → document → `sl-wire`'s decoder →
  `LslSyntax` equals the table.

Acceptance: a viewer logged into the fake grid fetches a non-empty
`LSLSyntax` document; the editor highlights and completes from it; the
version-gate and no-id cases are covered by fixtures; and the round trip
is a unit test.
