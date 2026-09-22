---
id: viewer-lsl-editor-widget
title: LSL editor widget — a parley PlainEditor fork with coloured ranges
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-lsl-script-editor
blocked_by: [viewer-ui-text-foundation, viewer-ui-text-input-widget]
refs: [viewer-ui-skin-tokens, viewer-notecard-editor, viewer-notecard-inline-items]
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

**"Per-range colour" is the wrong target; per-range *style* is the right one.**
[[viewer-lsl-editor-highlight]] settled that a range should carry a **token
class** and let the cascade resolve it, because `bevy_flair` maps far more
than colour onto a text node — `font-weight`, `font-style`, `font-width`,
`letter-spacing`, `line-height`, `text-decoration-line` /
`-color`, `text-shadow`, the font features and variations. Bold keywords,
italic comments and an underlined error range are all stylesheet decisions,
and a design that can only carry a `Color` per range forecloses every one of
them. Parley's brush list is the mechanism; what a brush *holds* is this
task's choice.

**Decide here: does a range ever get a box?** Per-token **background** cannot
come from the cascade — `background-color` maps to `BackgroundColor`, a UI
**node** component, and an inline range is a `TextSpan`. So a
selection-behind-a-token or highlighted-region effect has to be drawn by this
widget. The current-line highlight below is the same question in its easiest
form; answer it once, for both.

Reuse what Bevy/parley already give: cursor and word/line motion, selection
geometry, IME, clipboard, bidi and grapheme-correct backspace. Build here:
undo/redo, per-range style, a gutter and line numbers, current-line highlight,
find/replace and go-to-line (for the error list). The style *source* (lexer
tokens) and the structural affordances (folding, brace match, outline) layer on
top in [[viewer-lsl-editor-highlight]].

Reference (Firestorm, read-only): `llscripteditor`, `llpreviewscript`,
`llviewertexteditor`.

## Parity-audit addendum (2026-08-19)

Add the script editor's **find/replace** floater
(`floater_search_replace.xml`): find next/previous, replace one/all,
case-sensitivity toggle, operating on the editor buffer.

## Half of it is built (2026-09-13)

[[viewer-notecard-inline-items]] needed the same two things and built them
rather than a second design: the workspace's **parley fork** gained
`PlainEditor::set_range_styles` (a style property scoped to a byte range —
which is what a token colour is) and `set_inline_boxes`, and the **Bevy fork**
gained `ComputedTextBlock::set_entities` so an editable field's per-range brush
resolves to a real `TextColor` and `Underline`. Over those sits
`sl-viewer-ui-widgets`' `ui_rich_text` field, with style classes and inline
objects already working and tested.

So the "fork it or overlay it" decision above is settled, and settled the way
this task recommended. What is left here is the **editor** over that field:
undo/redo, a gutter and line numbers, current-line highlight, find/replace and
go-to-line, plus the benchmark of a 64 KB buffer against `PlainEditor`'s
whole-buffer relayout — which the notecard, whose bodies are small, never had to
answer.
