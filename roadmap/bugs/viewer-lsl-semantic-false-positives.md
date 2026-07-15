---
id: viewer-lsl-semantic-false-positives
title: LSL semantic pass false-positives on legal scripts (found by the tailslide oracle at scale)
topic: viewer
status: bugs
origin: found by the differential-testing oracle (2026-07-15)
refs: [viewer-lsl-differential-testing, viewer-lsl-semantic-pass, viewer-lsl-parser-tree]
---

Context: [context/viewer.md](../context/viewer.md).

Once the parser stack-overflow ([[viewer-lsl-parser-recursion-stack-overflow]])
was fixed, the differential oracle ([[viewer-lsl-differential-testing]]) could
finally run to completion over tailslide's full 187-script
`tests/scripts/` corpus (`SL_LSL_DIFFTEST_CORPUS`). It reported **12 genuine
false positives** — scripts tailslide (and therefore the grid) compiles
cleanly but `sl-lsl` flags with an error, violating the no-false-positive bar.
(A 13th, `parserstackdepth2.lsl`, is the *accepted* depth-limit divergence
documented in [[viewer-lsl-parser-recursion-stack-overflow]], not a real bug.)

Both sides used the **same** library table (built from tailslide's own
`builtins.txt`), so these are real semantic/grammar gaps, not a library-version
mismatch. The committed corpus stays at zero false positives; this is only
visible at scale.

The offending scripts, with the rough gap each exposes:

- **`print(...)`** (`print_expression.lsl`, `fpinc.lsl`) — LSL's legacy `print`
  is a real (void) expression the library table does not carry, so it reads as
  an undefined call.
- **Postfix/prefix `++`/`--` on a `float` and on a member component**
  (`fpinc.lsl`: `x++` on a float, `(string)v.x++`, `++v.x`) — component-lvalue
  and float increment are mistyped.
- **Nested vector/rotation grammar** (`vconst.lsl`:
  `<1,2,<1,1,1>*<1,1>1,1> >`) — the `<`/`>` vs comparison disambiguation.
- **Labels / jumps** (`duplicate_labels.lsl`, `jump_annotations.lsl`,
  `lso_jump_behavior.lsl`) — scoping/duplicate rules for `@label` / `jump` that
  are stricter than the grid's.
- **Key inlining / constant folding cases** (`key_inlining.lsl`, `vconst.lsl`,
  `lsl_conformance.lsl`, `parser_abuse.lsl`) — assorted expression forms the
  pass rejects that the grid accepts.

Each should be reduced to a minimal case, added to the committed
`sl-lsl/tests/corpus/valid/` (so it becomes a standing zero-false-positive
guard), and the responsible check in `analyze` (or the parser) relaxed to match
the grid. Re-run the oracle at scale (`SL_LSL_DIFFTEST_CORPUS`) after each to
confirm the false-positive count drops without introducing a miss.
