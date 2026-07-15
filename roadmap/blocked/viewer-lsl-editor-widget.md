---
id: viewer-lsl-editor-widget
title: LSL editor widget — a parley PlainEditor fork with coloured ranges
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-lsl-script-editor
blocked_by: [viewer-ui-text-foundation, viewer-ui-text-input-widget]
refs: [viewer-notecard-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The real work of an in-viewer LSL editor is the **text widget**, and Bevy 0.19
fights it. Bevy's `EditableText` **is** `parley::PlainEditor` — which means it
inherits *one style for the whole buffer* and **no undo**. Worse,
`bevy_ui_render`'s editable-text path hard-codes the glyph section index to 0
and paints every glyph with a single `TextColor`. **So stock Bevy 0.19
physically cannot render more than one colour inside an editable text field.**

- **Fork it** (recommended, everything is MIT/Apache): vendor `parley`'s editor
  plus Bevy's editable-text layout/render path — roughly 2k lines — and add the
  two things missing: a **per-range brush list** (parley's `RangedBuilder`
  already supports it; `PlainEditor` merely doesn't expose it) and an
  **undo/redo** stack (easy over a `String` buffer). Ongoing cost: re-diff
  against Bevy each release, and its text internals are actively churning.
- **Or overlay** as a cheap MVP: a transparent stock `EditableText` for
  caret/selection with a coloured non-editable rich `Text` drawn behind it. Zero
  forks, but two independent layouts must agree pixel-for-pixel — plausible with
  a monospace font and no wrapping, unverified with font fallback.

**Benchmark before committing:** `PlainEditor` relays out the *whole buffer* on
every edit, and nobody has published numbers for a 64 KB script. If it is too
slow the fallback is one `Layout` per source line (code does not wrap) — but
parley's `Selection` works within a single `Layout`, so that means writing
multi-line cursor logic ourselves. This choice is hard to reverse; make it
first. No rope is needed (`PlainEditor` itself uses a `String`, and 64 KB is
small).

**Design it for a second consumer.** [[viewer-notecard-editor]] needs the same
widget *plus* **inline boxes** — a notecard embeds inventory items inline in the
text. Parley supports inline boxes already, so one rich-text editor can serve
both; but "per-range colour" and "inline objects plus per-range colour" are
different designs, and it is much cheaper to know that before writing the first
one than after.

Reuse what Bevy/parley already give: cursor and word/line motion, selection
geometry, IME, clipboard, bidi and grapheme-correct backspace. Build here:
undo/redo, per-range colour, a gutter and line numbers, current-line highlight,
find/replace and go-to-line (for the error list). The colour *source* (lexer
tokens) and the structural affordances (folding, brace match, outline) layer on
top in [[viewer-lsl-editor-highlight]].

Reference (Firestorm, read-only): `llscripteditor`, `llpreviewscript`,
`llviewertexteditor`.
