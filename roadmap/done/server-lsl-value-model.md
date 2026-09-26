---
id: server-lsl-value-model
title: The LSL value model — seven types and their exact coercions
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
refs: [server-lsl-compiler-ir, server-lsl-lib-strings-lists]
---

Context: [context/lsl.md](../context/lsl.md). Design:
`book/src/simulator/lsl-engine.md` ([[server-lsl-architecture]]).

This task **creates the `sl-lsl-runtime` crate** (its `README.md` points
at that chapter) and puts the value half there. The compile-time half of
the rules below — which casts and operator/operand combinations are
errors, and each legal one's result type — belongs in `sl-lsl` as a pure
table over `ast::TypeName`, where the semantic pass and the lowering both
read it; write it there, and test the two halves against each other.

Before anything can be executed there has to be a value. LSL's type
system is small and almost entirely made of special cases, and every one
of them is observable from a script, so this is a pure, heavily
unit-tested crate module with no world behind it — the cheapest large
piece of the programme to get exactly right.

The types: `integer` (**32-bit, wrapping**, not saturating), `float`
(**`f32`**, not `f64` — a script that adds 0.1 a hundred times sees `f32`
drift and content depends on it), `string`, `key` (a string that is not a
string: `(key)"" == NULL_KEY` is false, an invalid key casts to `FALSE`
in a boolean test), `vector` and `rotation` (`f32` components, and
`sl_types::lsl::Vector` / `Rotation` already exist), and `list` — ordered,
heterogeneous, and **flat**: a list cannot contain a list, which is why
the semantic pass already reports tailslide's `E10034`.

What has to be pinned, each with a test:

- **The cast matrix.** Every `(type)value` pair, including the ones that
  are errors at compile time (`(key)integer`, tailslide's `E10035`) and
  the ones that silently succeed. `(integer)"0x1A"` reads hex;
  `(integer)"  12abc"` reads `12`; `(integer)"abc"` is `0`, never an
  error. `(string)` of a float is `%.6f`, of a vector
  `<%.6f, %.6f, %.6f>`, of a rotation four components — the exact
  formatting content parses back out with `llCSV2List`.
- **Arithmetic.** Integer division by zero is a **run-time error**, float
  division by zero is not. `integer % 0`. Mixed `integer op float`
  promotes. `vector * vector` is the dot product, `vector % vector` the
  cross product, `vector * rotation` rotates, `vector / rotation` rotates
  by the inverse, `rotation * rotation` composes — and composes in
  Linden's order, which is the reverse of glam's
  (see the `llquaternion-composes-backwards` note).
- **Boolean context.** What is false: `0`, `0.0`, `""`, `NULL_KEY` and an
  invalid key, the zero vector/rotation (they are **not** usable in a
  condition at all — a compile error), and the empty list.
- **Comparison.** `list == list` compares **lengths**, not contents —
  the single most surprising rule in the language, and one scripts rely
  on. String comparison is byte-wise and case-sensitive; `<` and `>` on
  strings are compile errors.
- **`+` overloads.** `list + anything` appends; `anything + list`
  prepends; `string + string` concatenates; `key + string` is a compile
  error.
- **No short-circuit.** LSL evaluates **both** operands of `&&` and `||`;
  a script written with a null-guard on the left is a documented
  footgun. Pin this against the reference before implementing it — it is
  exactly the kind of rule a Rust implementation gets "right" by accident
  and thereby wrong.

Oracle: `~/devel/3rdparty/LSL-PyOptimizer/lslopt/lslbasefuncs.py` (its
`typecast`, arithmetic and comparison helpers are a precise, tested
implementation of these rules) and
`opensim/.../ScriptEngine/Shared/LSL_Types.cs`. Where the two disagree,
Second Life wins and the divergence is recorded in the test.

Acceptance: an `LslValue` type with the full cast matrix, the operator
set, and boolean/comparison rules, each covered by a table-driven test
whose expected values are quoted from one of the two oracles with the
source named in a comment. No `Host`, no world, no I/O.

## Done (2026-09-27)

- **`sl-lsl::types`** — the compile-time table: `binary_result`,
  `prefix_result`, `postfix_result`, `assign_result` (with the
  `integer *= float` quirk), `cast_legal`, `implicitly_converts` and
  `ALL_TYPES`. Transcribed from tailslide's `OPERATOR_RESULTS` /
  `LEGAL_CAST_TABLE` / `COERCION_TABLE`, and the tests compare every
  combination against that transcription. The semantic pass now types
  arithmetic when both operand types are known, and its private
  `compatible` became `implicitly_converts`. Two differential-corpus
  scripts pin it (`valid/operator_result_types.lsl`,
  `error/operator_result_mismatch.lsl`); the tailslide oracle agrees on
  all 20 files with no false positives.
- **`sl-lsl-runtime`** (new crate) — `Value` / flat `Element`,
  `Value::is_true`, `cast`, `binary`, `prefix`, the `parse` and `format`
  modules, `ValueError`. Every rule has a table-driven test quoting
  PyOptimizer's `lslbasefuncs.py` or its `unit_tests/expr.suite`
  (`casts`, `operators`, `operators-compare`, `math-error`,
  `nan-fcast-vcast-minus0`), and a sweep checks the value half accepts
  exactly what the table accepts and yields the type it states.

Where the oracle corrected this task's text (SL wins, as the task says):

- float division by zero **is** a run-time `Math Error` (as are
  `vector / 0.0` and a NaN float quotient);
- the zero vector and rotation **are** usable in a condition (they are
  false), as is every other type;
- `(string)vector` has **five** decimals (six only inside a list), and a
  float prints seven significant digits, not plain `%.6f`;
- `list != list` is the length difference, `string != string` is 0/1, and
  `<=`/`>=` are `!(>)`/`!(<)`, so NaN makes them true.

One divergence from Second Life is recorded, not fixed: SL composes
`<3,5,7,17> * <.22,.26,.38,.86>` with `y == 8.32`, PyOptimizer's formula
(followed here) gives `8.320001`, and no summation order tried reproduces
SL. The short-circuit question is settled at the type level (`&&`/`||`
are integer-only, both operands evaluated, right to left) and left to the
lowering, which is where evaluation order lives.
