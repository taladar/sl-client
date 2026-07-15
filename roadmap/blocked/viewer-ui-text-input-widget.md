---
id: viewer-ui-text-input-widget
title: Reusable text-input widget (EditableText + IME preedit)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

A reusable single- and multi-line text-input widget wrapping `bevy_ui_widgets`'
`EditableText`, with **IME preedit rendering** — winit hands us a single preedit
cursor range, while the reference models composition as clause segments plus
standout flags, so budget real time for the display. This is the widget every
text-entry surface consumes: chat input, IM, search fields, the rebinding
editor, notecard/script editors.

The caret and selection API must be expressed in **logical** terms
(`forward`/`backward`, not left/right) and be bidi-correct via `parley`'s
visual-order cluster movement and multi-rect `selection_geometry()` — the
groundwork proven in [[viewer-ui-text-foundation]]. Grapheme-correct editing
(`backdelete()`) is inherited from parley.

Out of scope here (its own task): the syntax-highlighted, per-range-styled,
undo-capable editor for [[viewer-lsl-editor-widget]] — `parley::PlainEditor` is
*plain* (one whole-buffer style set), a gap in parley itself, so that editor
builds on `RangedBuilder` + `editing::{Cursor,Selection}` separately.

Reference (Firestorm, read-only): `lllineeditor`, `lltexteditor`, `llpreeditor`.
