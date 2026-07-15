---
id: viewer-lsl-lexer
title: LSL lexer — the logos token stream (sl-lsl)
topic: viewer
status: done
origin: user request (2026-07); split from viewer-lsl-parser
---

Context: [context/viewer.md](../context/viewer.md).

The first piece of a new **pure crate** (`sl-lsl`: no Bevy, no I/O, tested with
plain `cargo test`, mirroring `sl-prim` / `sl-anim` / `sl-avatar`): a **`logos`
lexer** that turns LSL source into a token stream. **Its only dependency is
`sl-types`** — a lexer has no business knowing about circuits or capabilities,
and keeping it clean means it can be tested, fuzzed and reused (a linter, a CI
check, an external tool) without a grid.

Classify comments, strings, numbers and operators, and emit every *word* as a
plain identifier token — the lexer does **not** bake in the library. Word
colouring and library lookups happen a layer up
([[viewer-lsl-editor-highlight]]) against the grid-provided keyword table
([[protocol-lsl-syntax]]), because the grid hands us the function list at
runtime.

The scanner is inherently **error-tolerant**, which is what an editor needs: it
re-lexes *broken* code on every keystroke. This is a non-problem for
performance — the script size limit is 65,536 characters and a 64 KB re-lex is
microseconds, so incremental lexing is unnecessary.

**Tree-sitter was surveyed and rejected** (2026-07). Every existing LSL grammar
— tree-sitter and TextMate/Sublime alike — **enumerates the ~500 library
functions as grammar literals** (one compiles to a 9.4 MB `parser.c`), which is
exactly backwards for a language whose symbol table the grid serves at runtime;
and the licences are unusable (the only maintained grammar is GPL-3.0, the
best-engineered one has no licence, the canonical Sublime grammar none either).
A hand-written scanner over the same token stream the editor already produces is
cheaper and carries no C toolchain.

This one token stream is shared: both the highlighter
([[viewer-lsl-editor-highlight]]) and the parser ([[viewer-lsl-parser-tree]])
consume it. Do not grow a second lexer.

Reference (Firestorm, read-only): `llkeywords.cpp` — a small hand-written
scanner classifies comments, strings, numbers and operators, and every word is
coloured by a hashmap lookup against the grid keyword table.
