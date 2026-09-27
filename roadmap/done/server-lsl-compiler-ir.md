---
id: server-lsl-compiler-ir
title: Lower the LSL syntax tree to something executable
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 13
blocked_by: [server-lsl-architecture, server-lsl-value-model]
refs: [server-lsl-vm-execution, viewer-lsl-oracle-misses]
---

Context: [context/lsl.md](../context/lsl.md). Design:
`book/src/simulator/lsl-engine.md` — a stack bytecode, one span per
instruction, library calls resolved to a `BuiltinId`. The semantic pass's
`expr_type` deliberately leaves arithmetic unknown; the lowering needs a
*complete* typing, built on the type table [[server-lsl-value-model]]
puts in `sl-lsl`.

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
  tailslide and the local OpenSim before choosing. (PyOptimizer, measured
  against SL, says operators evaluate **right to left** — `a - b + c`
  evaluates `c`, `b`, `a` — see the `sl_lsl::types` module docs.)
- **The type table is done.** `sl_lsl::types` gives every operator's,
  assignment's and cast's result type (`integer *= float` included) and
  `sl-lsl-runtime`'s `cast` / `binary` / `prefix` are the value half; the
  lowering reads the one and emits the other.
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

## As built (2026-09-27)

`sl_lsl_runtime::compile(source) -> Result<Program, Vec<CompileError>>`;
the design and its guarantees are in the book chapter
`simulator/lsl-engine.md`, "As built: the compiler". In short:

- **Three stages** — parse (first syntax error only), the semantic pass
  against `library::lsl_syntax()` (the runtime's own table as an
  `LslSyntax`), then the lowering (`compiler/lower.rs`), which types and
  resolves completely and enforces every rule the pass misses, after
  tailslide's passes. All 19 of the pass's remaining oracle misses
  ([[viewer-lsl-oracle-misses]]) are compile errors here.
- **E10019 decided: an error.** "Not all code paths return a value" is
  tailslide's `final_pass.cc` rule, which PyOptimizer also raises: the last
  statement of a value function must be a `return` or an `if`/`else` whose
  branches both end in one.
- **Evaluation order pinned** from tailslide's Mono back end: binary operands
  right to left (left operand on top for `Binary`), arguments, list elements
  and vector components left to right. `state` leaves the body at once, as
  Mono's `ChangeState; ret` does.
- **Messages are Linden's** (PyOptimizer's `EParse*` texts), grammar-level
  mistakes are `Syntax error`, and `CompileError`'s `Display` is the wire
  form with **zero-based** positions, as Second Life sends them.
- **Debug printer**: `Program`'s `Display` disassembles with names resolved
  and a `line:column` per instruction; the unit tests pin it.
- **Acceptance**: `tests/compile_corpus.rs` with tailslide as the oracle —
  the committed corpus is exact (every accepted script compiles, every
  rejected one errs on a line tailslide names), and tailslide's own 187
  scripts show no false rejection, no miss and no error elsewhere, bar two
  known divergences reported apart (the parser's depth ceiling, and
  functions missing from the vendored table:
  [[server-lsl-library-table-refresh]]).

**Confirmed live on aditi** (2026-09-27, two probe scripts run in stock
Firestorm): the evaluation order above exactly (`f(1) - f(2) + f(3)` → 3,
2, 1; lists, vectors and arguments left to right; `&&` both sides, right
first); `c + 5 + e *= 4` as `c + 5 + (e *= 4)` (`a=14 e=8`); a nested
`return` of a void call compiles and runs; `integer *= float` truncates
(`3 *= 1.5` → 4). E10019 is a compile error, reported as `(4, 0): ERROR :
Not all code paths return a value` for a function whose closing brace is on
line 5 — so positions are zero-based and point at the **closing brace** (not
the name, as tailslide has it); the lowering now does the same. And using
the value of `integer *= float` (`float g = i *= 1.5;`) compiles, but the
handler then fails with `System.InvalidProgramException: Invalid IL code …
stloc.s 9` before its first line: the lowering marks such a body with a
leading `InvalidProgram` instruction for the VM to raise.

Found and fixed on the way, both in `sl-lsl`, each with a corpus case:

- **The parser gave `=` to the wrong target.** `c + 5 + e *= 4` parsed as
  `(c + 5 + e) *= 4`; the grid's grammar has an lvalue, not an expression, on
  an assignment's left, so it is `c + 5 + (e *= 4)` (tailslide's
  `tltp/exporter.lsl`). `parse_operand` now absorbs an assignment after a bare
  variable or member.
- **The semantic pass rejected a legal `return`.** Returning a void call from
  a void function or event is legal nested in an `if` or loop (tailslide's
  `void_return.lsl`); the pass now tracks the nesting.

Also found: the client documents and shows grid compile-error positions as
one-based — [[viewer-script-compile-error-positions-zero-based]].
