---
id: viewer-lsl-parser
title: LSL parser — a pure crate (sl-lsl)
topic: viewer
status: ideas
origin: user request (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

A new **pure crate** (`sl-lsl`: no Bevy, no I/O, tested with plain
`cargo test`, mirroring `sl-prim` / `sl-anim` / `sl-avatar`) that lexes and
parses LSL into a tree.

**Self-contained: its only dependency is `sl-types`.** Not `sl-proto`, not
`sl-wire`, nothing from the session stack — a parser has no business knowing
about circuits or capabilities, and keeping it clean means it can be tested,
fuzzed and reused (a linter, a CI check, an external tool) without a grid.
`sl-types` already carries an `lsl` module, so the value types the language
needs (`vector`, `rotation`, `key`, list) have a home rather than being
reinvented here.

It exists because the grid's syntax data and the grid's compiler each leave a
gap that only a parser fills:

- **[[protocol-lsl-syntax]] knows the *library*, not the *script*.** It tells us
  every `ll*` function and constant — but nothing about the user's own globals,
  functions, `state` blocks, event handlers or locals. Go-to-definition,
  find-references, rename, an outline of states/events, folding, brace matching,
  auto-indent and scope-aware completion all need a parse tree.
- **The grid compiler costs an upload — and a reset.** SL has no
  compile-without-save: compilation happens *as part of* the upload. So every
  "did I typo that function name?" is a slow network round-trip **and** a
  mutation of the world — the in-world script is replaced, its state resets, and
  a live vendor or attachment misbehaves while you iterate. The point of a local
  parser is not to replace the compiler but to **collapse the edit loop**: catch
  the ordinary mistakes instantly and side-effect-free, so a save happens when
  you believe the script is right, not as a way of finding out.

## Scope

- **Lexer + parser** producing an **error-tolerant** tree. This is the part that
  differs from a compiler front-end: an editor parses *broken* code on every
  keystroke, so recovery matters more than a clean grammar, and the tree must
  survive a half-typed statement.
- **The language is small**, which is what makes this tractable: C-like syntax,
  seven types (`integer`, `float`, `string`, `key`, `vector`, `rotation`,
  `list`), global variables and functions, `state` blocks containing event
  handlers, no classes, no generics, no user preprocessor. **Hand-written:** a
  `logos` lexer plus recursive descent.
- **Tree-sitter was surveyed and rejected** (2026-07). Every existing LSL
  grammar — tree-sitter and TextMate alike —
  **enumerates the ~500 library functions as grammar literals** (one compiles to
  a 9.4 MB `parser.c`), which is exactly backwards for a language whose symbol
  table the grid serves at runtime; and the licences are unusable (the only
  maintained grammar is GPL-3.0, the best-engineered one has no licence, the
  canonical Sublime grammar none either). Writing our *own* tree-sitter grammar
  would be possible, but then it buys only structure — for which a hand-written
  parser over the same token stream the editor already produces is cheaper and
  carries no C toolchain. Share the lexer with [[viewer-lsl-script-editor]]; do
  not grow two.
- **Semantic pass — and it can be held to a real standard.** LSL's type rules
  are small enough to check locally (arity and types at call sites, using the
  grid's function signatures from [[protocol-lsl-syntax]]; undefined symbols;
  unreachable states; missing `return`). Because SL has
  **no compile-without-save**, this is the *only* way to type-check without
  mutating the world, so it earns its keep.

  The bar is high, though: a false error on code the grid would happily compile
  is worse than no error at all. What makes that bar reachable is that
  **Linden Lab's own front-end is public** — `tailslide` reproduces the legacy
  bytecode **byte-for-byte**, so its lexing, typing and implicit-conversion
  quirks are the real ones. Use it as a **differential-testing oracle**: run
  tailslide and `sl-lsl` over a corpus and diff the diagnostics, rather than
  hoping we matched by reading. (A local OpenSim serves the same role for
  grid-side truth.)

  Do not over-claim, though — three things stay authoritative on the server: the
  **Mono/CIL** path is only *semantically equivalent* to the legacy bytecode, so
  the two can diverge exactly where a local check would;
  **OpenSim compiles LSL to C#** with its own quirks and messages; and several
  failures are not front-end errors at all (script too large, no modify
  permission, experience not permitted, upload failure). Local checking makes
  the edit loop fast — it never replaces the save.

  **Luau/SLua is a separate language** — scope this crate to LSL and let Luau
  have its own parser if it ever matters.
- **Diagnostics worth reading — the other half of the value.** LSL's compiler
  errors are terse to the point of hostility (`(12, 5) : ERROR : Syntax error`,
  and little else). Owning the parser *and* holding the grid's typed signatures
  lets us do modern, rustc-grade diagnostics instead: a labelled span with the
  source excerpt and a caret (ariadne / miette / codespan-style), **"did you
  mean…?"** by edit distance against the grid's real function table (so it
  suggests `os*` functions on OpenSim automatically), and honest type errors —
  *"`llSetTimerEvent` expects `(float Rate)`, got `string`"* — quoting the
  tooltip the grid already gave us.

  Apply the same treatment to the **server's** errors: `ScriptCompileError`
  already carries line and column, so re-render the simulator's authoritative
  diagnostic through our own span machinery. Even errors only the grid can
  produce then arrive with a caret and context instead of a bare line number.
- Prior art worth reading before designing the grammar:
  **`secondlife/tailslide`** — Linden Lab's own **MIT-licensed**, actively
  maintained LSL parser / AST / compiler (descended from the community's
  `lslint`). It is the authority on real LSL semantics and the edge cases a wiki
  grammar will miss. Read it; do not bind to it (a C++ FFI would only pay off if
  we wanted to generate bytecode in the viewer, which the protocol does not need
  — the simulator compiles).

Consumed by [[viewer-lsl-script-editor]] (highlighting structure, folding, brace
matching, outline) and [[viewer-lsl-language-server]] (symbols, navigation,
local diagnostics). Neither *blocks* on it — both can ship with grid-provided
highlighting alone — but both get substantially better with it, and it is the
only way to get anything smarter than a keyword table.
