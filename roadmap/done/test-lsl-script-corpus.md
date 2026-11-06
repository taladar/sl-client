---
id: test-lsl-script-corpus
title: An executable LSL corpus that runs in cargo test with no grid
topic: test
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [server-lsl-vm-execution]
refs: [test-lsl-differential-opensim, server-lsl-lib-strings-lists]
---

Context: [context/lsl.md](../context/lsl.md).

`sl-lsl` already has three test corpora — `tests/lex.rs`,
`tests/parse.rs`, `tests/semantics.rs` — plus the differential run
against tailslide over `tests/corpus/`. None of them *run* anything.
The runtime needs its own, and it should be the cheapest test in the
workspace: source in, expected output out, no grid, no network, no
tokio.

Shape:

- a directory of `.lsl` files, each paired with an expected transcript —
  the ordered list of everything the script did through the mock host
  (`say`, `set_pos`, `set_timer`, `sleep`, …) plus the final value of
  its globals;
- a **mock `Host`** that records calls and answers queries from a tiny
  stated scene, so a script can be exercised without `sl-fake-grid` at
  all — which is the whole reason [[server-lsl-architecture]] puts the
  world behind a trait;
- a driver that runs a script for a bounded number of ticks and diffs
  the transcript, with `UPDATE_EXPECT`-style regeneration so adding a
  case is writing the script and running the test once;
- cases seeded from the places that already have worked examples: the
  SL wiki's per-function examples, `LSL-PyOptimizer`'s
  `unit_tests/expr.suite`, and OpenSim's own script tests.

This is also where the semantics that have no grid-side observable get
pinned: `&&` evaluating both sides, `list == list` comparing lengths,
integer wrap-around, `llGetSubString`'s wrapping indices, the
evaluation order of call arguments. A transcript test states them as
*behaviour* rather than as a unit test of an internal helper, which is
what makes it survive a rewrite of the VM.

Audit 2026-10-15: the runtime crate is `sl-lsl-runtime`; its only
integration test is `tests/compile_corpus.rs` (compile errors against
tailslide). The VM's unit tests (`src/vm/tests.rs`) already have a
recording `Host` (`Recorder`) to grow the mock host from; the `.lsl` +
transcript directory and its driver do not exist.

Acceptance: `cargo test -p sl-lsl-runtime` runs the corpus in under a
second with no grid; a deliberately changed expectation fails with a
readable diff; and every semantic rule listed in
[[server-lsl-value-model]] has at least one transcript case.

## Done (2026-11-06)

`sl-lsl-runtime/tests/script_corpus/` runs every `corpus/<area>/<name>.lsl`
in the VM against a recording host (one prim alone in a region, its owner
present) and diffs the transcript against `<name>.expected`, or
`<name>.<flavour>.expected` where the three engines disagree.
`UPDATE_EXPECT=1` writes them. The whole corpus runs in about 0.01 s, a
changed expectation fails with one `pretty_assertions` diff naming every
engine it failed on, and an expectation with no script fails too. The format
is in the book, `simulator/lsl-engine.md` § As built: the transcript corpus.

36 cases in six areas: `values` (every rule of [[server-lsl-value-model]]:
the casts, a key that is no string, `f32` drift and where Mono rounds,
boolean context, list and string comparison, the `+` overloads, the vector
and rotation operators, integer wrap-around), `errors` (the Math Errors,
worded and handled three ways), `evaluation` (aditi's measured order:
operands right to left, `&&`/`||` both sides, assignment as an expression),
`strings` and `lists` (`llGetSubString`'s wrapping indices among them), and
`events` (timers, sleeps, state changes, touches, listens, link messages,
`on_rez`, a setter's `changed`, `llDie`, speech volumes and a capped
`llSetPos`). Seeds: PyOptimizer's `expr.suite`, the SL wiki, the book's
aditi measurements, and OpenSim's own `LSL_TypesTestLSLInteger` table and
`LSL_ApiListTests.TestllListFindList`.

The host is the test's own, not a public mock in the library: the `Host`
trait and its types are public already, so the corpus needed no test-only
API. The cases skip lslint (`.gitattributes`): they pin exactly what it
warns about, and it cannot parse a cast of a cast.

Two things the transcripts showed, both already the runtime's measured
behaviour and now pinned as such: inside one expression a float stays
double until something stores it (`big + 1.0 == big` is false, the stored
sum equal), and XEngine raises a timer a tick after the other engines.
