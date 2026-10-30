---
id: server-lsl-library-unserved-functions
title: Five functions no served LSLSyntax document lists
topic: server
status: done
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

## Done (2026-10-30)

- **Measured.** The `script-compile` census uploads one call each
  (`UNSERVED`, in a `touch_start` nobody raises) under every backend and
  holds the answers. Aditi compiles **all five** under Mono and LSL2. Both
  OpenSim engines compile `llPointAt`, `llStopPointAt` and
  `llRemoteLoadScript` and refuse the two experience functions as undefined.
  Rows in `book/src/gridspec/lsl.md` § Running, re-uploading, sizes, targets.
- **The table.** `sl-lsl-runtime/keywords_lsl_unserved.xml` holds the five
  in the served document's format (a `functions` map only): tailslide's
  signatures and argument names, `llRemoteLoadScript`'s 3 s delay from LSL
  PyOptimizer's `fndata.txt` (flagged deprecated: `llRemoteLoadScriptPin`
  replaced it), energy 10. `build.rs` merges it and stops when a function is
  in both documents, or the supplement holds anything but functions.
  Provenance in the crate `README.md`.
- **Tests.** The independent-decoder test compares the served part only; a
  new one holds the supplement to tailslide's signatures and checks no served
  document has them. `compile_corpus.rs`'s module comment says the "not in
  the library table" list is expected empty.
- **Follow-ups.** [[gridspec-lsl-unserved-experience-probe-steps]] (the two
  experience functions in a probe, which the earlier tasks left out until
  they compiled here); [[viewer-lsl-editor-flags-unserved-functions]] (the
  editor checks against the served document and marks all five undefined);
  a note on [[server-fake-grid-script-compile-on-upload]] that `FakeOpensim`
  should refuse the experience two.
