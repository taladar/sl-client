---
id: server-lsl-library-surface-table
title: One generated table for the library, and a coverage harness over it
topic: server
status: done
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

## Done (2026-09-27)

- **Source:** Linden's own `LSLSyntax` document,
  `keywords_lsl_default.xml`, vendored at the `sl-lsl-runtime` crate root
  (Firestorm `684bc1d1`, last upstream change `secondlife/viewer#1744`;
  LGPL-2.1 like the workspace; SHA-256 and the reasons over tailslide's
  `builtins.txt` and OpenSim's `ScriptSyntax.xml` in the crate README).
  428 functions, 709 constants, 38 events.
- **Generated at build time** (`sl-lsl-runtime/build.rs`, parsed with
  `sl-llsd`): `BuiltinId`, `BUILTINS` (name, typed args, return, sleep,
  energy, flags, tooltip), `CONSTANTS` (typed values; the `EOF` and
  `JSON_*` notations decoded), `EVENTS`, and a `Signature` per function.
  Lookups `builtin` / `constant` / `event` by name.
- **Dispatch:** typed implementations registered with `registry!`, held
  to the generated `Signature` at compile time through a generic
  `Handler` over argument tuples; stubs check arguments against the
  descriptor and return the type's default, flagged `stubbed` so the VM
  can say so once.
- **Coverage:** `status(id)`, and a test printing
  `3 implemented / 0 stubbed / 425 missing of 428` that fails only when a
  function falls back from `src/library/coverage.txt`.
- **Proof of the pipeline:** `llAbs`, `llFabs` (keeps `-0.0` and a
  negative NaN, per PyOptimizer) and `llGetListLength` implemented.
- **Cross-check:** a test decodes the same file through `sl-wire`'s
  independent `LSLSyntax` decoder and compares every function, event and
  constant with the generated table — which found a real `sl-wire` bug,
  fixed here: constant values served as `<uuid>` (`NULL_KEY`,
  `TEXTURE_*`) decoded to no value.

Differences from the design sketch, recorded in the book chapter: an
unregistered function is `Missing` at run time rather than a compile
error, and the call context is a type parameter until the VM task shapes
`ScriptCtx` / `Host`. The `LSLSyntax` rendering itself stays with
[[protocol-sim-lsl-syntax-document]], now unblocked.
