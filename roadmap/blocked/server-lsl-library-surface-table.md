---
id: server-lsl-library-surface-table
title: One generated table for the library, and a coverage harness over it
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [server-lsl-architecture]
refs: [protocol-sim-lsl-syntax-document, server-lsl-lib-strings-lists]
---

Context: [context/lsl.md](../context/lsl.md).

Around 425 `ll*` functions, 35 events, several hundred constants. Split
across a dozen tranche tasks and written over months, the failure mode is
not difficulty, it is **drift**: a function implemented with the wrong
arity, a constant with the wrong value, a tranche believed complete that
is missing four functions nobody noticed, and a second hand-kept list
(the `LSLSyntax` document the grid serves) disagreeing with the first.

So the table comes first and everything reads from it.

Wanted:

- **One machine-readable source** of name, argument types, return type,
  energy and sleep, plus the constants and their values and the events
  and their parameters. Two ready-made inputs exist locally:
  `~/devel/3rdparty/tailslide/builtins.txt` (1334 lines, generated from
  Linden's own derived-files generator) and
  `~/devel/3rdparty/opensim/bin/ScriptSyntax.xml` (a complete `LSLSyntax`
  document, which is also the *output* format). Pick one as the import,
  vendor it with its provenance, and record which — the licence position
  of each needs checking before vendoring.
- **Generated Rust**: the descriptor table and a dispatch enum, so a
  library function cannot be registered under a signature the table does
  not state, and a call with the wrong arity is a compile error in *our*
  code rather than a run-time surprise.
- **A coverage test** that fails with a list: which functions have an
  implementation, which are deliberate stubs (returning the type's
  default and logging once), and which are absent. This is the
  programme's progress bar and the thing that makes "is the library
  done?" answerable without reading sixteen task files.
- **One consumer already waiting**: the grid's own `LSLSyntax` document
  ([[protocol-sim-lsl-syntax-document]]) should be *rendered from this
  table*, so the viewer's highlighter and autocomplete see exactly the
  functions the engine implements — which is strictly more honest than
  serving a copy of Linden's list for an engine that implements half of
  it.

Acceptance: the table generated at build time from a vendored source
with its provenance recorded; a coverage test printing
`implemented / stubbed / missing` counts and failing only on a
*regression* in that split; and the descriptor used by the dispatch so
arity and types cannot drift from it.
