---
id: server-lsl-library-unserved-functions
title: Five functions no served LSLSyntax document lists
topic: server
status: ready
origin: server-lsl-library-table-refresh (2026-10-26)
points: 2
refs: [server-lsl-library-table-refresh, gridspec-lsl-compile]
---

Context: [context/lsl.md](../context/lsl.md).

The library table is now the `LSLSyntax` document Second Life serves, and
it leaves out five functions tailslide's `builtins.txt` lists: `llPointAt`,
`llStopPointAt`, `llRemoteLoadScript`, `llClearExperiencePermissions` and
`llGetExperienceList`. A script calling one fails to compile here.

Wanted: find out on aditi whether the grid still compiles each — the
`gridspec-lsl-compile` census uploads sources without compiling them
locally, so it can carry five one-call scripts — and add the ones it
compiles to the table from a second source (tailslide's signatures, the
viewer's older document for sleep and energy) through `build.rs`, kept
apart from the vendored document so a refresh stays a copy. The
independent-decoder test then compares the vendored document's part only.
Afterwards the scale run of `tests/compile_corpus.rs` lists none of them as
"not in the library table", and its module comment says so.
