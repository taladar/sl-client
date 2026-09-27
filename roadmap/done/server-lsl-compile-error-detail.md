---
id: server-lsl-compile-error-detail
title: Compile errors that say what is wrong, not only Linden's bare message
topic: server
status: done
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

## As built (2026-09-27)

`CompileError` now carries a `message` beside its `kind`; `Display` keeps the
wire shape, `(line, column): ERROR : message`, zero-based, with the message
in place of Linden's text (`CompileErrorKind::message()` still gives
Linden's words for the same situation). Every error site in the lowering
names its specifics:

- **Names**: `no function `llOwnerSya`; did you mean `llOwnerSay`?` — the
  suggestion drawn from what is in scope (locals, globals, user functions,
  states, labels) and the library, through `sl_lsl::closest`; a function or
  state used as a variable, a variable called as a function, and a global
  used above its declaration each say so.
- **Types**: `the initial value of `i` must be `integer`, but this is `float`;
  an explicit `(integer)` cast converts it` — every implicit-conversion site
  (initialiser, assignment, argument, return value, vector component) names
  the value it is checking; operators name both operand types
  (`there is no `key + string``, with a hint for the common cases), compound
  assignments what the result would be.
- **Calls**: the signature, with the library's parameter names —
  `` `llSay(integer Channel, string Text)` takes 2 arguments, but 1 was
  given ``.
- **Structure**: the event's real signature, which state already handles an
  event, what a global initialiser may be, why a label is out of scope.

The semantic pass is **no longer a stage** of `compile`: the lowering
reports every situation it did (by the same or stricter rules — it is the
pass that is conservative), with better suggestions, so keeping both only
mixed two message styles. `library::lsl_syntax()`, which existed for that
stage, is gone; the served `LSLSyntax` document task can bring a renderer
back. The tailslide corpus runs are unchanged by it: the committed corpus is
exact, and tailslide's 187 scripts show no false rejection and no miss.
