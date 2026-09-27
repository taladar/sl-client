---
id: server-lsl-compile-error-detail
title: Compile errors that say what is wrong, not only Linden's bare message
topic: server
status: ready
origin: user feedback on server-lsl-compiler-ir (2026-09-27)
points: 3
refs: [server-lsl-compiler-ir, server-fake-grid-script-compile-on-upload]
---

Context: [context/lsl.md](../context/lsl.md).

`sl_lsl_runtime::compile` rejects scripts in the same situations as Second
Life and at the same positions, but says only what Linden's compiler says —
`Type mismatch`, `Name not defined within scope`, `Function call mismatches
type or number of arguments`. The user's call (2026-09-27): matching *when*
an error happens is what matters; the wording may and should be more
helpful.

Wanted: `CompileError` carries the specifics the lowering already knows —
the undefined name, the expected and found types, the operator and operand
types, the function and argument index, the label or state — and its
message names them (e.g. `` `llSay` takes (integer, string) but was given
(vector, string) ``), reusing `sl-lsl`'s rendering where it fits (did-you-
mean against the library, the signature quoted back). Keep the kind enum,
so tests and the grid's "same situation" guarantee stay checkable, and keep
the wire shape `(line, column): ERROR : message` with zero-based positions,
which the viewer parses. The semantic pass's `DiagnosticKind` already has
these fields, so its errors can pass them through.
