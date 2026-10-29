---
id: viewer-lsl-editor-highlight
title: LSL editor highlighting — colour, folding, brace match, outline
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-lsl-script-editor
blocked_by: [viewer-lsl-editor-widget, viewer-lsl-lexer]
refs: [protocol-lsl-syntax, viewer-ui-skin-tokens,
  viewer-i18n-colorblind-accessibility, viewer-skin-panel-text-roles,
  viewer-lsl-editor-completion, viewer-syntax-scheme-choice]
---

Context: [context/viewer.md](../context/viewer.md).

Drive the editor widget's per-range colour ([[viewer-lsl-editor-widget]]) from
the lexer's token stream ([[viewer-lsl-lexer]]), and add the structural
affordances that fall out of the same stream: **brace matching, folding,
auto-indent and a states/events outline**.

The reference viewer already shows the right shape (`llkeywords.cpp`): the
scanner classifies comments, strings, numbers and operators, and every *word* is
coloured by a **hashmap lookup against the grid-provided keyword table**
([[protocol-lsl-syntax]]). That is why the library list must **not** be baked in
— the grid hands us the functions at runtime, including OpenSim's OSSL, so a new
function colours correctly with no code change. A 64 KB re-lex is microseconds
(the script limit is 65,536 characters), so re-highlight-on-keystroke is a
non-problem.

Anything deeper — go-to-definition on user functions, rename, scope-aware
completion — wants a real tree ([[viewer-lsl-parser-tree]]) and belongs to the
language server, not here. This task stays at what the token stream alone can
do.

**Autocomplete and signature help** come nearly free from the grid's syntax data
(each function carries its return type, typed arguments, tooltip, and its energy
and sleep cost). Firestorm has *no* autocomplete and *no* brace matching — this
is open goal, not parity work.

## Where the colours live (decided 2026-09-22)

**Their own token group, not their own mechanism.** A syntax scheme and a UI
skin are different choices — every code editor treats them that way, and a
resident who wants Solarized in the script editor is not asking for a
different chat window — so the token names live apart (`--syntax-keyword`,
`--syntax-comment`, `--syntax-string`, `--syntax-number`,
`--syntax-event`, …) and a skin that defines none of them still works.

But they go through the **same `common.css` / role-token plumbing** as
everything else, for three reasons that a separate config file would give up:

- A **colour-blind or high-contrast overlay** has to reach them. Syntax
  colour is the densest colour-as-signal surface in the viewer, so
  [[viewer-i18n-colorblind-accessibility]] needs them addressable by the same
  mechanism as `--gain` / `--loss` / `--notice-*`.
- A **light skin needs light-appropriate code colours** or the editor is the
  one window that looks wrong. Keeping them in the cascade lets a skin ship a
  matching scheme *if it wants to*, while not requiring it.
- The machinery already exists. A separate file would mean a second parser, a
  second hot-reload path and a second fallback story, for a set of about
  fifteen values.

So: a skin **may** override the group; a user **may** pick a scheme
independently of the skin; neither has to know about the other. The fallback
sheet carries the default scheme the way it carries every other role's value.

The practical shape this implies for the widget: the editor's per-range
appearance wants to be driven by a **token class per range** rather than a
resolved `Color`, so the cascade does the resolving — the same move
[[viewer-skin-panel-text-roles]] made for panel text, and for the same reason
(a class beats a Rust-painted `TextColor`, so mixing the two silently wins one
way).

A class also buys far more than the colour, which is the better argument for
it. `bevy_flair` maps, per text node: `color`, `font-weight`, `font-style`,
`font-width`, `font-feature-settings`, `font-variation-settings`,
`letter-spacing`, `line-height`, `text-decoration-line`,
`text-decoration-color` and `text-shadow`. So **bold keywords, italic
comments, an underlined error range and a dimmed folded region are all
stylesheet decisions** — none of which a `Color` per range could ever express,
and all of which a scheme author will want.

**One exception, and it is worth knowing before the widget is designed:**
per-token **background** is *not* available this way. `background-color` maps
to `BackgroundColor`, a UI **node** component, and an inline range is a
`TextSpan` rather than a node — so a highlighted-line or
selection-behind-a-token effect needs the editor to draw it, not the cascade.
That is a constraint on [[viewer-lsl-editor-widget]]'s per-range design (the
fork's brush list is per-*glyph* colour, not a box), so decide there whether
ranges ever get a box of their own.

Reference (Firestorm, read-only): `llscripteditor`, `llkeywords` (the token
table), `llsyntaxid`.

## Measured (2026-10-26, [[gridspec-lsl-ossl]])

Two things to know before this is built:

- **Nothing delivers the grid's document to the editor yet.** The fetch,
  cache and decode are done ([[protocol-lsl-syntax]]): `Event::LslSyntax`
  arrives, and `sl-client-bevy` only logs it (`sl-client-bevy/src/lib.rs`,
  `SessionEvent::LslSyntax`). Keeping the decoded table as a resource the
  editor (and, later, `sl-lsl-lsp`, whose binary starts with
  `LslSyntax::default()`) can read is part of this task.
- **OpenSim's document is not what the region allows.** The one the local
  grid served lists 224 `os*` functions and four `ls*` ones with no word
  about who may call them — `osAvatarStopAnimation` and `osConsoleCommand`
  (disabled by the stock `[OSSL]` configuration) and `osTerrainSetHeight`
  (denied by threat level) read like `osGetRegionSize` — and it leaves out
  `osListenRegex` and every `mod*` function, which compile and run. So OSSL
  colours as the document says, not as the region behaves; a denial shows
  only at run time, as `DEBUG_CHANNEL` chat ([[viewer-script-warning-window]]).

## Offline check (from server-fake-grid-lsl-syntax, 2026-10-28)

The fake grid now serves each flavour's live document
([[server-fake-grid-lsl-syntax]]), so the acceptance check that the editor
colours and completes from the grid's table can run offline: a viewer logged
into `sl-fake-grid` gets Linden's document on the Second Life flavour and
OpenSim's, with its `os*` functions, on the OpenSim one.

## Done (2026-10-29)

- **A language-neutral model**, the new pure crate `sl-highlight`: highlight
  captures in **tree-sitter's vocabulary** (`keyword`, `function.builtin`,
  `string.escape`, …, hierarchical so `function.builtin` falls back to
  `function`), bracket pairs (a closer pairs with the nearest opener of its
  kind, so an unclosed `(` does not break its block's braces), folds (from
  after the opener to the start of the closing line, so the closer stays in
  view) and an outline. A Lua highlighter fills in the same structure and every
  scheme colours it unchanged; LSL adds only `function.event` (event handlers)
  and uses `namespace` for states.
- **The LSL producer**, `sl_lsl::highlight::structure`, off the lexer's token
  stream alone: words by the **grid's table** first, then the grammar's own
  keywords and types (so a grid with no table still colours control flow),
  then neighbours (the word after `state` is a state, after `@` / `jump` a
  label, an unknown word before `(` the script's own function); string escapes
  split out; deprecated library symbols flagged; `{ }`, `[ ]` and block
  comments that span lines fold; the outline found by brace depth (globals,
  functions, states and their handlers).
- **The editor** (`sl-viewer-ui-widgets` `ui_code_syntax`): a
  `CodeHighlighter` (a function from buffer to structure) on the field is
  called in the frame the buffer changes, before the model reaches parley, so
  colour never lags. Each capture is a skinned class `sk-syntax-<capture>` read
  from `--syntax-<capture>`; deprecated symbols draw in
  `sk-syntax-deprecated`. **Brace match** boxes the pair at the caret (after
  first, then before) and an unpaired bracket. **Folding** hides the range
  (`RichTextStyle::Hidden`, so fully hidden lines collapse to no height), draws
  a `⋯` box in its place that unfolds on a click, puts `▾` / `▸` toggles in the
  gutter beside each fold's first line, and takes `Ctrl+Shift+[` / `]`; folds
  follow edits before them, unfold when an edit lands inside them, and the
  caret steps over a folded range in the direction it was moving. **Indent**:
  `Enter` keeps the line's indentation, one level deeper after an opener, and
  between a pair opens an indented line with the closer below; `}` typed in a
  line's indentation takes its partner's line's; `Tab` inserts a level (or
  indents every selected line) and `Shift+Tab` removes one — in a code editor
  `Tab` never moves focus.
- **The grid's table reaches the editor**: `GridLslSyntax` in
  `sl-viewer-asset-editors` keeps the last `LSLSyntax` document the session
  decoded, and every open script is highlighted again when it changes.
- **The outline**: a *Jump to…* list over the script editor (globals,
  functions, states and indented handlers) that puts the caret on the picked
  name.
- **Skins**: `--syntax-*` is its own group. The fallback sheet carries a dark
  scheme; Vintage, the light skin, ships its own; graphite and azure use the
  fallback's. `shipped_skins` exempts only `--syntax-*` from the
  every-skin-defines-every-token check. New washes `--bracket-match`,
  `--bracket-unmatched`, `--fold-placeholder` in every skin; the skins
  chapter documents all of it.

## Not done

- **Autocomplete and signature help** from the grid's table — split into
  [[viewer-lsl-editor-completion]].
- **A syntax scheme chosen apart from the skin** (the "a user may pick a
  scheme" half of the decision above) — split into
  [[viewer-syntax-scheme-choice]]. The cascade plumbing it needs is in place.
- The offline acceptance check against `sl-fake-grid` (colouring from each
  flavour's served table) is covered by unit tests of the highlighter against a
  table and of the editor against a highlighter, not by a logged-in viewer run.
