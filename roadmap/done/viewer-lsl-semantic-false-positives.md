---
id: viewer-lsl-semantic-false-positives
title: LSL semantic pass false-positives on legal scripts (found by the tailslide oracle at scale)
topic: viewer
status: done
origin: found by the differential-testing oracle (2026-07-15)
refs: [viewer-lsl-differential-testing, viewer-lsl-semantic-pass, viewer-lsl-parser-tree,
  viewer-lsl-oracle-misses]
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

## Done (2026-09-15)

Re-run first: the scale oracle (tailslide v1.5.9, 187 scripts) reported 13
false positives — the 11 genuine ones above plus `parserstackdepth2.lsl` and
its copy under `expected/`. The harness now prints each false positive's
**messages**, not only its line numbers, which is what split them into four
causes rather than the five guessed at above:

- **`print`** was a call to an undefined function. It is a keyword of the
  grid's grammar (`PRINT '(' expression ')'`), never in the library table, so
  it is now its own `Expr::Print` node, and a reserved word (`(string)print`
  is a syntax error, as there). Its type stays unknown: LL's compiler types it
  inconsistently. The `++`/`--` suspicion in `fpinc.lsl` was a red herring —
  its only error was the `print`.
- **Duplicate labels** are a warning on the grid (tailslide `E20017`), so
  `DuplicateLabel` is now `Severity::Warning`. `jump` resolution was already
  lenient enough; none of the three label scripts tripped `UndefinedLabel`.
- **Vector `<`/`>`**: the parser suppressed every `<`/`>`-spelled operator in
  every component. The grid's LALR tables (checked with `bison -v` on
  tailslide's `lslmini.y`, states 239/246/255) say otherwise: in a non-last
  component all of them are operators; in the last only `>` is ambiguous, and
  it compares when the next token can start an expression — except `-` and
  `<`, which the `%prec INITIALIZER` rule makes close the constructor. The
  parser now follows exactly that at the component's top level; deeper in
  (under a looser operator) it closes only when nothing could follow, which
  can accept what the grid rejects but never the reverse.
- **Scanner leniency** (`parser_abuse.lsl`): the grid's scanner skips any
  character that starts no token, treats a `"` with no closing quote as one
  such character, and accepts `L"…"` strings. The lexer learned the `L`
  prefix (an editor should colour it as a string too); the other two live in
  the parser's token pass (`grammar_tokens`), so the editor still colours an
  unterminated string to the end of the file.

`parserstackdepth2.lsl` is the accepted depth-ceiling divergence from
[[viewer-lsl-parser-recursion-stack-overflow]]; the harness now reports a
script that trips the ceiling on its own line instead of failing the run on
it, keyed on the parser's message text so a reworded message fails loudly.

Result at scale: clean-agree 100 (was 89), error-agree 44, missed 20,
**false-positive 0**, depth-ceiling 2. Two scripts moved from error-agree to
missed, and that is a correction: `list_append_void.lsl` and
`print_type_bug.lsl` only "agreed" because `print` was flagged as undefined;
tailslide's actual objection is a void value, which the pass does not check.
`print_no_shadowing.lsl` moved the other way, now that `print` is reserved.
The misses are filed as [[viewer-lsl-oracle-misses]].

Committed corpus guards (`sl-lsl/tests/corpus/valid/`), each confirmed to fail
on the old parser and to lint clean in tailslide: `print_expression.lsl`,
`duplicate_labels.lsl`, `vector_component_comparisons.lsl`,
`scanner_leniency.lsl`. Committed-corpus run: 18 files, 10 clean-agree,
8 error-agree, 0 missed, 0 false positives. Unit tests for each rule in
`tests/parse.rs`, `tests/lex.rs` and `tests/semantics.rs`.
