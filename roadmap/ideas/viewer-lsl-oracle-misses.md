---
id: viewer-lsl-oracle-misses
title: Grid compile errors the LSL semantic pass does not report (20 oracle misses)
topic: viewer
status: ideas
origin: scale oracle run while fixing
  [[viewer-lsl-semantic-false-positives]] (2026-09-15)
refs: [viewer-lsl-semantic-false-positives, viewer-lsl-differential-testing,
  viewer-lsl-semantic-pass]
---

Context: [context/viewer.md](../context/viewer.md).

With the false positives at zero, the scale differential run over tailslide's
`tests/scripts/` still lists **20 misses** — scripts tailslide rejects on which
`sl-lsl` is silent. A miss is tolerated by design (the pass is conservative),
but every one is a save that fails on the grid after the editor said nothing.
Grouped by tailslide's error:

- **Operator typing** (`E10002`): `string | string`, `string ++`,
  `integer << float`, `vector *= vector`, `list += <void call>` —
  `bugs/0003`, `bugs/0015`, `compound_assignment`, `list_append_void`. Needs
  the operand-type table for each operator, which `expr_type` deliberately
  leaves unknown today. The same gap hides `llOwnerSay(1 + 1)`
  (`type_error_no_assert`, `E10011`): the argument's type is never inferred,
  so the call check skips it.
- **Void values** (`E10011`, `E10015`): a void call passed to `print` or
  assigned to a variable — `print_type_bug`, part of `list_append_void`.
- **Assignment typing** (`E10015`): `string s = [..]`, `integer j = ".."` —
  `bugs/0017`.
- **Not all code paths return a value** (`E10019`) is an **error** there, and
  the pass reports `MissingReturn` as a warning — `check_all_return`,
  `retval`. Worth deciding whether to promote it, given tailslide mirrors the
  grid's compiler.
- **Lists in lists** (`E10034`) — `nested_lists`, `bugs/0019`.
- **Invalid member** (`E10008`): `v.j`, `r.house` — `bugs/0010`.
- **Casts**: `(key)integer` (`E10035`, `illegal_cast`); `(string)-x` without
  parentheses (`E10020`, `bugs/typecast_builtin`).
- **Declarations**: a declaration as an unbraced `if` body (`E10032`,
  `invalid_decl`); a non-constant global initialiser (`E10021`,
  `ll_global_rules`); a state with no event handler (`E10023`, `bugs/0002`).
- **Namespaces** (`E10001`, `E10005`, `E10026`): one name used as a variable
  and a state or function, a library function name declared as a variable —
  `scope1`, `scope2`.
- **Grammar** (`E10020`): `rvalue_assignments` (assignment to a non-lvalue).
  Not in the corpus but found alongside: a backslash before a line break
  inside a string (`"a\` then `b"`) ends no string on the grid — its string
  pattern's `\\.` does not cross a newline — while the lexer reads it as an
  escape and accepts the literal.

Each fix must keep the scale run at zero false positives; add the reduced case
to `sl-lsl/tests/corpus/error/`. Reproduce with `SL_LSL_TAILSLIDE_BIN` and
`SL_LSL_DIFFTEST_CORPUS` as in [[viewer-lsl-differential-testing]].
