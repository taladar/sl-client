---
id: viewer-lsl-editor-widget
title: LSL editor widget — a parley PlainEditor fork with coloured ranges
topic: viewer
status: done
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

## Done (2026-10-29)

The editor is `sl-viewer-ui-widgets`' `ui_code_editor`, over the rich-text
field, and the script editor's body is one (read-only scripts too, which can
now be searched and copied in the same editor rather than a static block).

- **Undo / redo** (`ui_text_history`): `Ctrl+Z`, `Ctrl+Y` / `Ctrl+Shift+Z` on
  any field carrying an `EditHistory` — every rich-text field, so the notecard
  editor has undo too. It records the buffer's *difference* after each frame's
  edits rather than hooking each kind of edit, so a paste, an IME commit, the
  overwrite mode and a Replace All are all undoable without knowing about each
  other. A run of typing (or of deleting) with no pause over a second and no
  line break is one step.
- **Per-range style, not colour.** A `RichTextClass` may name a **skin
  class** (`RichTextClass::skinned`): its section node wears the class and the
  cascade resolves it — colour, underline, and the `font-weight` /
  `font-style` the field reads back into the run, so bold keywords and italic
  comments are a stylesheet's decision.
- **Ranges get boxes — answered once, for both questions.** A field now has an
  **underlay** behind its glyphs, and `RichTextBackdrop`s (a byte range plus a
  skin class) are drawn there, one box per laid-out line the range touches,
  coloured by the cascade's `background-color`. The caret line is the same
  mechanism (`--caret-line`), and so are find matches (`--find-match`, through
  `RichTextMarks`, which a widget writes beside the consumer's content). Only
  what is on screen gets a box. For anything to sit between the well and the
  glyphs the well had to move off the field: a rich-text field now wears it on
  its root, a `FieldBox` that stamps `.sk-focus-within` / `.sk-read-only` from
  its field (new `.sk-field-box.sk-read-only` rule).
- **Gutter and line numbers**, placed from parley's own lines so they stay
  level with the text through scrolling and a taller fallback glyph; wheel
  scrolling over text and gutter; monospace and unwrapped.
- **Find / replace** — the reference's Search / Replace window (Search,
  Replace, Replace All, Case Insensitive, Search Up), `Ctrl+F` / `Ctrl+H`, `F3`
  / `Shift+F3` to repeat, seeded from a one-line selection; every match on
  screen is washed while the window is up. **Go to line**: `CodeEditorGoTo`
  for callers, `Ctrl+G` and a Go to Line window for the resident, and the
  script editor's compile errors are clickable and jump the caret to the line
  and column they name. Both windows are also buttons beside Save.
- Tokens `--caret-line`, `--find-match`, `--gutter-bg`, `--line-number` in the
  fallback sheet and every skin (dark tints in Vintage's light well).

### The benchmark, and what it led to

`sl-viewer-ui-widgets/tests/editor_relayout.rs` (ignored timings, run by hand
in release). `PlainEditor` relaid the whole buffer out on every keystroke,
linear in its size — 5.4 ms at 16 KB, 11 ms at 32 KB, **22 ms** at the 64 KB
script limit with a style on every token, over a 60 Hz frame.

So the per-line fallback the task anticipated was done, but inside the parley
fork rather than as a list of line layouts (which would have meant writing the
multi-line caret, selection and hit testing parley already has): the editor
keeps each paragraph's broken layout keyed by its text and the styles, boxes and
preedit rebased to it, re-shapes only the paragraphs an edit touched, and joins
them into the one `Layout` everything else works on (fork item 5 in the root
`Cargo.toml`). Edits also move the range styles and inline boxes with the text;
without that the relayout an edit forces saw every later paragraph as
restyled. Tests in the fork compare every line, glyph, box and caret position
against a whole layout, through edits. A keystroke now, with the highlighter's
ranges recomputed over the whole buffer:

| script | lines | plain | highlighted | first layout |
| --- | --- | --- | --- | --- |
| 16 KB | 677 | 0.14 ms | 0.64 ms | 15 ms |
| 32 KB | 1,366 | 0.27 ms | 1.3 ms | 26 ms |
| 64 KB | 2,731 | 0.76 ms | 3.6 ms | 52 ms |

Text needing bidi resolution (an Arabic or Hebrew comment or string) still lays
out whole: parley resolves bidi over the whole text with one base level, which
a paragraph laid out alone cannot reproduce. Not timed: Bevy's own pass over
the finished glyphs on each change.

### Not done here

- A **scrollbar** for a scrolled text field: [[viewer-text-field-scrollbar]]
  (the editor scrolls by wheel and by the caret).
- `Tab` inserting indentation, auto-indent, brace match, folding and the
  colours themselves: [[viewer-lsl-editor-highlight]].
