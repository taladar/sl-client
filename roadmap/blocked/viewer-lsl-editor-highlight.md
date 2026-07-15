---
id: viewer-lsl-editor-highlight
title: LSL editor highlighting — colour, folding, brace match, outline
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-lsl-script-editor
blocked_by: [viewer-lsl-editor-widget, viewer-lsl-lexer]
refs: [protocol-lsl-syntax]
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

Reference (Firestorm, read-only): `llscripteditor`, `llkeywords` (the token
table), `llsyntaxid`.
