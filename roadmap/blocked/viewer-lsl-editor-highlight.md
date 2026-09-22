---
id: viewer-lsl-editor-highlight
title: LSL editor highlighting — colour, folding, brace match, outline
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-lsl-script-editor
blocked_by: [viewer-lsl-editor-widget, viewer-lsl-lexer]
refs: [protocol-lsl-syntax, viewer-ui-skin-tokens,
  viewer-i18n-colorblind-accessibility, viewer-skin-panel-text-roles]
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
