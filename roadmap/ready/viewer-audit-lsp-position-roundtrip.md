---
id: viewer-audit-lsp-position-roundtrip
title: The LSP position-to-byte direction has zero tests
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-lsl-lsp/src/position.rs:160` — `LineIndex::offset_at` has **no tests**,
while its inverse `position()` has four (including a multibyte emoji case).

`offset_at` is the direction every cursor request routes through —
`hover.rs:29`, `completion.rs:40`, `signature.rs:40`, `inlay.rs:41`, and
`navigate.rs:34`, `:56`, `:82`, `:159` — and LSP-position-to-byte is the classic
home of UTF-16-versus-byte bugs.

Missing:

- a round-trip property test asserting
  `offset_at(text, position(text, off, enc), enc) == off` for every char
  boundary of a document containing an emoji (2 UTF-16 units), a BMP non-ASCII
  char, and CRLF lines, across every negotiated encoding;
- clamping cases: a character past the line end, a line past EOF, a column
  landing inside a surrogate pair;
- a `tests/server.rs` case that opens a document with non-ASCII text and asks
  for hover or definition on a **later** line.

For the record, `sl-lsl`'s own coverage is strong and should be left alone —
`tests/lex.rs` (18) covers maximal munch, slash disambiguation, unterminated
string and block comment, and exact spans on char boundaries; `tests/parse.rs`
(30) covers error recovery specifically, including
`parser_never_panics_on_operator_soup` and
`deeply_nested_input_terminates_without_stack_overflow`; `tests/differential.rs`
runs a tailslide oracle over a committed corpus. The thin spots there are
`src/render.rs` (743 lines, 9 tests) and `src/syntax.rs` (323 lines, 1 test).

Perf note for the same file set: `sl-lsl-lsp/src/navigate.rs:35`, `:57`, `:82`,
`:159` each call `resolve(&document.parse().script, syntax)` from scratch, so
four requests on one cursor position re-walk the whole tree four times; nothing
caches the occurrence list on `Document`.
