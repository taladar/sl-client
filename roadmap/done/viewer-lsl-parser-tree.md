---
id: viewer-lsl-parser-tree
title: LSL parser — error-tolerant recursive-descent AST (sl-lsl)
topic: viewer
status: done
origin: user request (2026-07); split from viewer-lsl-parser
blocked_by: [viewer-lsl-lexer]
---

Context: [context/viewer.md](../context/viewer.md).

A **recursive-descent parser** over [[viewer-lsl-lexer]]'s token stream that
builds an **error-tolerant** tree. This is the part that differs from a compiler
front-end: an editor parses *broken* code on every keystroke, so recovery
matters more than a clean grammar, and the tree must **survive a half-typed
statement** rather than bail on the first syntax error.

**The language is small**, which is what makes this tractable: C-like syntax,
seven types (`integer`, `float`, `string`, `key`, `vector`, `rotation`,
`list`), global variables and functions, `state` blocks containing event
handlers, no classes, no generics, no user preprocessor. `sl-types` already
carries an `lsl` module, so the value types the language needs (`vector`,
`rotation`, `key`, list) have a home rather than being reinvented here.

Why a tree at all: [[protocol-lsl-syntax]] knows the *library*, not the
*script*. It tells us every `ll*` function and constant but nothing about the
user's own globals, functions, `state` blocks, event handlers or locals.
Go-to-definition, find-references, rename, an outline of states/events, folding,
brace matching, auto-indent and scope-aware completion all need this parse tree.

The AST is consumed by the semantic pass ([[viewer-lsl-semantic-pass]]) and the
language server ([[viewer-lsl-lsp-server]]).

**Luau/SLua is a separate language** — scope this crate to LSL and let Luau have
its own parser if it ever matters.

Reference (Firestorm, read-only): `secondlife/tailslide` — Linden Lab's own
MIT-licensed LSL parser / AST / compiler (descended from the community's
`lslint`), the authority on real LSL grammar and the edge cases a wiki grammar
will miss. Read it; do not bind to it.
