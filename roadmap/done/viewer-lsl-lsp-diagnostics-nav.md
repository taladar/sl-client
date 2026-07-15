---
id: viewer-lsl-lsp-diagnostics-nav
title: LSL language server — diagnostics, navigation, completion
topic: viewer
status: done
origin: user request (2026-07); split from viewer-lsl-language-server
blocked_by: [viewer-lsl-lsp-server, viewer-lsl-semantic-pass]
---

Context: [context/viewer.md](../context/viewer.md).

The language-intelligence half of the LSP server ([[viewer-lsl-lsp-server]]):
**diagnostics**, **go-to-definition**, **find-references**, **rename** and
**scope-aware completion**. Navigation and rename come from the parse tree's
symbol table; completion, hover docs and signature help come from the grid's
syntax data ([[protocol-lsl-syntax]]); the diagnostics come from the local
semantic pass ([[viewer-lsl-semantic-pass]]) plus the grid's authoritative
`ScriptCompileError`.

**The catch to design around: there is no compile-without-save on SL.**
Compilation happens *as part of the upload*, so "diagnostics on save" has a side
effect on the grid — you cannot silently type-check a buffer. Decide the
semantics explicitly:

- grid diagnostics only on an explicit save/compile action, not on every
  keystroke;
- make it obvious *which* in-world script a buffer maps to before it is
  overwritten;
- the client-side parser ([[viewer-lsl-semantic-pass]]) gives cheap syntax and
  type diagnostics with no grid round-trip, leaving the authoritative semantic
  errors to the grid compiler.

The grid data makes some genuinely nice features cheap: warn on `deprecated`
functions, flag god-mode-only calls, surface sleep/energy cost inline (LSL
performance is dominated by exactly these — available as LSP inlay hints), and
complete event names within the right `state` block. The one LSP nobody else can
build: ours takes its symbols from the connected grid, so it sees OpenSim's OSSL
that a static function list cannot.
