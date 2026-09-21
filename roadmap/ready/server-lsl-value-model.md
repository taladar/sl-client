---
id: server-lsl-value-model
title: The LSL value model — seven types and their exact coercions
topic: server
status: ready
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
refs: [server-lsl-compiler-ir, server-lsl-lib-strings-lists]
---

Context: [context/lsl.md](../context/lsl.md).

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
