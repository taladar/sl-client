---
id: viewer-lsl-script-editor
title: LSL script editor
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-framework, viewer-prim-inventory-editing, protocol-lsl-syntax]
---

Context: [context/viewer.md](../context/viewer.md).

An in-viewer LSL script editor: syntax highlighting, tooltips, compile / save
via caps, a clickable error list, and the object-contents workflow (open a
script from a prim's inventory, edit, save back).

Almost all the protocol is already done: upload (`UpdateScriptAgent` /
`UpdateScriptTask`), target selection (we support `Lsl2` / `Mono` / **`Luau`** —
Firestorm only has two), run / reset / query, and
**`ScriptCompileError`, already parsed into line, column and message** — which
is strictly better than Firestorm, whose error navigation is a `sscanf` that
assumes a `(line, col)` prefix. So a clickable error list that jumps the caret
is nearly free. The missing protocol piece is [[protocol-lsl-syntax]].

## Highlighting: a hand-written lexer, not a grammar (surveyed 2026-07)

**Do not use tree-sitter or syntect here**, tempting as they look. Every
existing LSL grammar — tree-sitter *and* TextMate/Sublime —
**bakes the ~500 library function names into the grammar itself** (one such
grammar compiles to a 9.4 MB `parser.c`), so adding a function means
regenerating the parser. That is precisely backwards for LSL, where
**the grid hands us the function list at runtime** ([[protocol-lsl-syntax]]).
The licences are also a minefield: the only maintained tree-sitter LSL grammar
is GPL-3.0, the best-engineered one has **no licence at all**, and the canonical
Sublime grammar is likewise unlicensed.

The reference viewer already shows the right shape (`llkeywords.cpp`): a small
hand-written scanner classifies comments, strings, numbers and operators, and
every *word* is coloured by a **hashmap lookup against the grid-provided keyword
table**. That is ~300–500 lines of Rust (a `logos` lexer, or by hand), it is
inherently error-tolerant, and a 64 KB re-lex is microseconds — the script size
limit is 65,536 characters, so incremental lexing is a non-problem.

Brace matching, folding, auto-indent and a states/events outline all fall out of
the same token stream. Anything deeper — go-to-definition on user functions,
rename, scope-aware completion — wants a real tree, which is
[[viewer-lsl-parser]] (and the reason to build it as its own crate rather than
growing a second grammar here).

## The widget: the real work, and a Bevy constraint

Bevy 0.19's `EditableText` **is** `parley::PlainEditor` — which means it
inherits *one style for the whole buffer* and **no undo**. Worse,
`bevy_ui_render`'s editable-text path hard-codes the glyph section index to 0
and paints every glyph with a single `TextColor`. **So stock Bevy 0.19
physically cannot render more than one colour inside an editable text field.**
Plan for that up front:

- **Fork it** (recommended, everything is MIT/Apache): vendor
  `parley`'s editor plus Bevy's editable-text layout/render path — roughly 2k
  lines — and add the two things missing: a per-range brush list (parley's
  `RangedBuilder` already supports it; `PlainEditor` merely doesn't expose it)
  and an undo/redo stack (easy over a `String` buffer). Ongoing cost: re-diff
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
first.

**Design it for a second consumer.** [[viewer-notecard-editor]] needs the same
widget *plus* **inline boxes** — a notecard embeds inventory items inline in the
text. Parley supports inline boxes already, so one rich-text editor can serve
both; but "per-range colour" and "inline objects plus per-range colour" are
different designs, and it is much cheaper to know that before writing the first
one than after.

No rope is needed (`PlainEditor` itself uses a `String`, and 64 KB is small).
Reuse what Bevy/parley already give: cursor and word/line motion, selection
geometry, IME, clipboard, bidi and grapheme-correct backspace. Build: undo/redo,
per-range colour, gutter and line numbers, current-line highlight, brace match,
find/replace, auto-indent, and go-to-line (for the error list).

## Scope calls

- **Autocomplete and signature help** come free from the grid's syntax data
  (each function carries its return type, typed arguments, tooltip, and its
  **energy and sleep cost**). Firestorm has *no* autocomplete and *no* brace
  matching — this is open goal, not parity work.
- **Skip the Firestorm preprocessor in v1.** It is boost::wave (a full C
  preprocessor) plus custom sugar, it changes what is actually stored in-world,
  and it forces a source-map so compile errors still point at your real lines.
  If it ever happens, it must ship the line map and the round-trip encoding.

Reference (Firestorm, read-only): `llscripteditor`, `llpreviewscript`,
`llkeywords` (the token table), `llsyntaxid`, `llfloaterscriptdebug`,
`fslslpreproc`. Also `secondlife/tailslide` — Linden Lab's own MIT-licensed LSL
parser/compiler, the best reference for real LSL semantics.

Deps: [[viewer-ui-framework]], [[viewer-prim-inventory-editing]] (opening a
script from a prim's contents), [[protocol-lsl-syntax]] (the keyword table).
