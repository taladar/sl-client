---
id: server-lsl-float-expression-precision
title: Round a float expression once, where Mono stores it, not at every operator
topic: server
status: ready
origin: server-lsl-memory-sizes (2026-10-30)
refs: [server-lsl-memory-sizes, server-lsl-value-model, server-lsl-compiler-ir,
  gridspec-lsl-strings-math]
---

Context: [context/lsl.md](../context/lsl.md).

`math/rotation-bits` on aditi (`probes/results/aditi/math/rotation-bits.txt`,
book `gridspec/lsl.md` § Precision) shows Mono evaluating an expression of
floats in **double** and rounding to a float only when the value is stored:
`a.x * b.x + a.y * b.y` in one statement and the same over two statements
(`p = a.x * b.x; t = a.y * b.y; s = p + t;`) differ in the last bit in three
of seven cases, and `a.x * b.x * a.y` against `p * a.y` in one. The VM's
`Value::Float` is an `f32` and every `Instr::Binary` rounds, so it reproduces
the two-statement form and not the one-expression form.

[[server-lsl-memory-sizes]] made the vector and rotation operators exact
(each is one library-side computation, rounded once or at every step as aditi
measured); this is the rest of the rule, which lives in the compiler and the
VM rather than in `ops.rs`.

## What to find out, then build

- **Where Mono rounds.** A store to a variable is measured. Still to probe on
  aditi (the `rotation-bits` bit printer can be reused): an argument passed to
  a user function or to a library function, a value put in a list, a cast
  (`(float)`, `(string)` of an unrounded sum), a comparison of an unrounded
  value, a vector or rotation built from expressions (`<a * b + c, …>`), a
  return value, a compound assignment (`s += a * b`) and `++` on a float.
- **Carry the double.** An `f64` operand kind on the VM's stack for float
  intermediates, rounded where the probe says Mono rounds — without changing
  `Value::Float` to `f64`, since every stored float is a float.
- **The operators that take a vector or rotation operand** (scaling a vector
  by a float, the dot product inside a longer expression) under the same rule.

## Acceptance

The `two-products` and `three-factors` rows of the aditi transcript, both
forms, are reproduced bit for bit by running the probe's own expressions in
the VM, and each newly measured rounding point has a row of its own.
