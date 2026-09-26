# sl-lsl-runtime

The runtime half of running **Linden Scripting Language** (LSL) scripts on
`sl-fake-grid`: values, lowering, a bytecode VM, the event queue and the
library, behind a `Host` trait the grid implements. The design record is the
book chapter `book/src/simulator/lsl-engine.md` — read it before adding to
this crate.

Like `sl-lsl`, which it builds on, the crate is **I/O-free, Bevy-free and
synchronous**. It depends on neither `sl-wire` nor `sl-proto`: turning a
script's `llSay` into a `ChatFromSimulator` is the grid's job.

## What is here so far

The **value model** — LSL's seven types and the rules that make them
observable from a script:

- `Value` and its flat list element, `Element` (a list cannot hold a list, so
  the type cannot express one);
- the **cast matrix** on actual values, including the lenient string parsers
  (`(integer)"0x1A"`, `(integer)"  12abc"`, `(vector)"<1,2,3"`);
- the **operators** — 32-bit wrapping integers, `f32` floats, vector dot and
  cross products, Linden-order rotation composition, list `+`, and the run-time
  `Math Error` on division by zero;
- **comparison** (a list compares by length, `!=` of two lists is the length
  difference) and **boolean context** (what a condition treats as false);
- **formatting**: Mono's seven-significant-digit float printing and the five-
  versus six-decimal vector forms.

The *compile-time* half — which combinations are legal and what type each
produces — is `sl_lsl::types`, and a test here runs every combination through
both and checks they agree.

The oracles are LSL PyOptimizer's `lslopt/lslbasefuncs.py` and its
`unit_tests/expr.suite` expected outputs, which were measured against Second
Life; each test names the one it quotes.
