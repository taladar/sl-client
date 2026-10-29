---
id: viewer-lsl-editor-completion
title: LSL editor autocomplete and signature help from the grid's table
topic: viewer
status: ready
origin: viewer-lsl-editor-highlight (2026-10-29)
refs: [viewer-lsl-editor-highlight, protocol-lsl-syntax, viewer-lsl-lsp-server]
---

Context: [context/viewer.md](../context/viewer.md).

The grid's `LSLSyntax` document (`GridLslSyntax` in
`sl-viewer-asset-editors`) carries every library function's return type,
typed arguments, tooltip, energy and sleep cost, every constant's type and
value and every event's signature. Firestorm offers no completion at all.

- **Completion**: while a word is typed in the script editor, a popup of the
  library symbols (and the script's own globals, functions and states from the
  outline) that start with it, filtered as typing goes on, `Enter` / `Tab` to
  accept, `Esc` to dismiss. The `:shortcode:` popup in `sl_viewer_chat` is the
  nearest existing shape.
- **Signature help**: inside a call's parentheses, the function's signature
  with the current argument emphasised, and its tooltip and sleep cost.
- **Hover**: a library symbol's tooltip under the pointer.

`Tab` is the code editor's indent key; accepting a completion with it must take
precedence only while the popup is open.
