---
id: gridspec-lsl-compile
title: Script compilation and compile errors on each grid
topic: gridspec
status: blocked
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
