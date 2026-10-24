---
id: gridspec-lsl-compile
title: Script compilation and compile errors on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-script-compile-error-positions-zero-based,
  viewer-lsl-editor-save-compile]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Evaluation order, `*=` quirks, zero-based positions measured on aditi; XEngine's
C#-style format known from source; YEngine, LSO and failed-save semantics
unmeasured.

## Discover

Upload deliberately broken sources per grid (and target), record `errors`
verbatim, whether a failed compile replaces the asset and the run flag, size
limits.

## Document

`book/src/gridspec/lsl.md` § Compiling.

## Fake grid

Large — [[server-fake-grid-script-compile-on-upload]].

## Viewer

Error list parses both formats, positions one-based.

## Done (2026-10-25)

- **Discover.** A census case, `script-compile` (Second Life and OpenSim):
  per backend — Second Life's `mono`, `lsl2` and `luau` targets, OpenSim's
  YEngine and XEngine and the `luau` target under YEngine — a valid script
  in a prim, sixteen broken sources over it each with its fault at a known
  place, sources of 65 000 bytes to 1 MiB, one with 60 KB of string
  constants, the running flag, the same source again over the running
  script, and a broken and a valid upload to an agent-inventory script. It
  records each answer verbatim (only UUIDs scrubbed), where our parse puts
  the first error, `new_asset`, the listed asset, and the stored source read
  back over the UDP transfer; every answer is held per grid. Second Life
  counts positions from zero in bytes and reports the token after the fault
  and one error only; OpenSim's engines count from zero in characters and
  report every error. A failed compile stores the broken source and stops
  the script on every grid. OpenSim compiles no agent-inventory upload and
  names the item's id as `new_asset`; Second Life hides a script's asset id
  even from its owner. No source limit to 1 MiB; LSL2 refuses large
  constants. **XEngine never answers an upload over a running script** (it
  stores and restarts it), and each such upload keeps an OpenSim HTTP
  handler until later uploads are refused — the census gives every other
  XEngine leg a script of its own.
- **Document.** `book/src/gridspec/lsl.md` § Compiling; a pointer from
  `content/scripts.md` § Uploading & compiling a script.
- **Fake grid.** Large, as planned:
  [[server-fake-grid-script-compile-on-upload]], now unblocked, carries the
  answers a flavour splits on.
- **Viewer.** `ScriptCompileError::parse` moves the grids' zero-based
  positions to one-based ([[viewer-script-compile-error-positions-zero-based]],
  verified on both grids by the census's `error_0_at` metrics), reads Luau's
  `:line:` form and XEngine's `-1` column, and keeps the column's unit, told
  from the format (Second Life's `(4, 20)` counts bytes, OpenSim's `(4,20)`
  characters); `character_column` puts it in characters for the rendered
  caret and the editor's error list, which lists a Luau error by its line.
  The tokio runtime's upload steps now time out after 60 s, as the bevy
  runtime's do, so a save XEngine leaves unanswered fails instead of
  waiting for ever.
