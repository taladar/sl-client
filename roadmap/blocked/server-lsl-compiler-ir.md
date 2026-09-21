---
id: server-lsl-compiler-ir
title: Lower the LSL syntax tree to something executable
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 13
blocked_by: [server-lsl-architecture, server-lsl-value-model]
refs: [server-lsl-vm-execution, viewer-lsl-oracle-misses]
---

Context: [context/lsl.md](../context/lsl.md).

`sl_lsl::parse` already produces a complete, fully-spanned `ast::Script`
— globals, user functions, states, event handlers, every statement and
expression form — and `sl_lsl::analyze` already resolves names against
the grid's symbol table and type-checks calls. The step that does not
exist is turning that tree into something a machine can run.

Wanted: a lowering pass producing whatever [[server-lsl-architecture]]
settled on (the recommendation there is a stack bytecode). What the pass
owes, beyond the obvious walk:

- **Resolution baked in.** Globals, locals and parameters become slot
  indices; user functions become call targets; library calls become
  table indices; states become state indices and `state foo;` a state
  index. The VM should never do a name lookup.
- **Implicit casts made explicit.** LSL inserts conversions at
  assignment, at argument passing, at return and in mixed arithmetic; the
  IR should carry them as nodes so the VM has no coercion logic of its
  own and [[server-lsl-value-model]] is the only place the rules live.
- **The evaluation-order rules that are not obvious.** Both operands of
  `&&` and `||` are evaluated. The order of evaluation within an
  expression and of arguments in a call is observable when a call has a
  side effect, and LSL's is not left-to-right everywhere — pin it against
  tailslide and the local OpenSim before choosing.
- **`jump` and labels**, including the LSL rules about jumping out of a
  block and a label's scope; `for` / `while` / `do`; `return` from a
  `void` function and the fall-off-the-end case (`E10019`, which
  [[viewer-lsl-oracle-misses]] is still deciding whether to promote).
- **A source map.** Every instruction carries the span it came from, so a
  run-time error reports a line ([[server-lsl-runtime-errors]]) and a
  future debugger can step. This is far cheaper to build in now than to
  retrofit.
- **Compile errors in the grid's own shape.** The output of a failed
  compile has to be `Vec<ScriptCompileError>` — line, column, message —
  which `sl-proto` already models and the viewer's editor already renders
  as a clickable list. The semantic pass's diagnostics are the source;
  the mapping from its `DiagnosticKind` to a grid-style message is this
  task's, and it should read like the grid's, because that is what
  [[server-fake-grid-script-compile-on-upload]] will serve.

The semantic pass is **conservative by design** (no false positives), so
it lets through code a real compiler rejects — the twenty known misses in
[[viewer-lsl-oracle-misses]]. The lowering must therefore not *assume* a
clean tree: a construct the pass allowed but the IR cannot represent is a
compile error raised here, not a panic.

Acceptance: every script in `sl-lsl/tests/corpus/` that tailslide accepts
lowers without error and every one it rejects produces at least one
`ScriptCompileError` with a plausible line; the IR round-trips through a
debug printer so a test can assert on it; and no `unwrap` in the pass.
