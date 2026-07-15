---
id: viewer-lsl-semantic-pass
title: LSL semantic pass — types, arity, undefined symbols, reachability
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-lsl-parser
blocked_by: [viewer-lsl-parser-tree, protocol-lsl-syntax]
---

Context: [context/viewer.md](../context/viewer.md).

A semantic pass over the parse tree ([[viewer-lsl-parser-tree]]) that checks the
things LSL's type rules make checkable locally: **arity and types at call
sites** (using the grid's function signatures from [[protocol-lsl-syntax]]),
**undefined symbols**, **unreachable states**, and **missing `return`**.

This earns its keep because **SL has no compile-without-save**: compilation
happens *as part of* the upload, so every "did I typo that function name?" is a
slow network round-trip **and** a mutation of the world — the in-world script is
replaced, its state resets, and a live vendor or attachment misbehaves while you
iterate. Local checking is the *only* way to type-check without mutating the
world; it collapses the edit loop so a save happens when you believe the script
is right, not as a way of finding out.

**The bar is high: a false error on code the grid would happily compile is worse
than no error at all.** Three things stay authoritative on the server, and the
pass must not claim to speak for them:

- the **Mono/CIL** path is only *semantically equivalent* to the legacy
  bytecode, so the two can diverge exactly where a local check would;
- **OpenSim compiles LSL to C#** with its own quirks and messages;
- several failures are not front-end errors at all (script too large, no modify
  permission, experience not permitted, upload failure).

Local checking makes the edit loop fast — it never replaces the save. Meeting
the no-false-positive bar is what [[viewer-lsl-differential-testing]] exists to
prove, diffing this pass against tailslide over a corpus.

The pass feeds the reader-facing diagnostics ([[viewer-lsl-diagnostics]]) and
the language server's navigation and completion
([[viewer-lsl-lsp-diagnostics-nav]]).
