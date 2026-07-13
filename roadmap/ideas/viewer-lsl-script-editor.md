---
id: viewer-lsl-script-editor
title: LSL script editor
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

An in-viewer LSL script editor: syntax highlighting, compile / save via caps,
script error / debug output, and the object-contents script workflow (open a
script from a prim's inventory, edit, save back). Optionally the Firestorm
preprocessor / shared script library conveniences.

Reference (Firestorm, read-only): `llscripteditor`, `llpreviewscript`,
`llfloaterscriptdebug`, `fslslpreproc` / `fsscriptlibrary` (preprocessor /
library).

Deps: [[viewer-ui-framework]], [[viewer-prim-inventory-editing]] (opening a
script from a prim's contents).
