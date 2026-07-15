---
id: viewer-lsl-diagnostics
title: LSL diagnostics — rustc-grade spans and did-you-mean
topic: viewer
status: done
origin: user request (2026-07); split from viewer-lsl-parser
blocked_by: [viewer-lsl-semantic-pass]
---

Context: [context/viewer.md](../context/viewer.md).

Turn the semantic pass's findings ([[viewer-lsl-semantic-pass]]) into
diagnostics worth reading — the other half of the crate's value. LSL's compiler
errors are terse to the point of hostility (`(12, 5) : ERROR : Syntax error`,
and little else). Owning the parser *and* holding the grid's typed signatures
lets us do modern, rustc-grade diagnostics instead:

- a **labelled span** with the source excerpt and a caret (ariadne / miette /
  codespan-style);
- **"did you mean…?"** by edit distance against the grid's real function table
  ([[protocol-lsl-syntax]]) — so it suggests `os*` functions on OpenSim
  automatically;
- honest type errors that quote the tooltip the grid already gave us — *"`
  llSetTimerEvent` expects `(float Rate)`, got `string`"*.

Apply the same treatment to the **server's** errors: `ScriptCompileError`
(already parsed into line, column and message — `sl-proto/src/types/script.rs`)
is re-rendered through this same span machinery. Even errors only the grid can
produce then arrive with a caret and context instead of a bare line number, and
the reused rendering means the local and grid-side diagnostics look identical to
the reader.

This is the shared rendering that both the in-viewer editor's error surfacing
([[viewer-lsl-editor-save-compile]]) and the language server
([[viewer-lsl-lsp-diagnostics-nav]]) present to the user.
