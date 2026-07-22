---
id: viewer-lsl-preprocessor
title: LSL preprocessor (FS-compatible)
topic: viewer
status: ideas
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-lsl-editor-widget, viewer-script-mirror-download]
---

Context: [context/viewer.md](../context/viewer.md).

A Firestorm-compatible LSL preprocessor in the `sl-lsl` stack: `#include`
(against inventory scripts and — our twist — the on-disk script mirror
tree of [[viewer-script-mirror-download]], so shared libraries live in
git), `#define` macros, and the FS extras (lazy lists, switch statements)
as far as compatibility with existing FS-preprocessed content demands.
Compiled output embeds the original source in a comment block for
round-tripping, exactly as FS does, so scripts survive editing in either
viewer.

Idea-stage questions: how much of the FS dialect is actually in use in
the wild; whether the LSP ([[viewer-lsl-editor-widget]] diagnostics)
should understand pre- or post-processed source.

Reference (Firestorm, read-only): `fslslpreproc` (a boost::wave-based
implementation), `panel_script_ed_preproc.xml`.
