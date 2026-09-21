---
id: test-lsl-script-corpus
title: An executable LSL corpus that runs in cargo test with no grid
topic: test
status: blocked
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

Acceptance: `cargo test -p <runtime crate>` runs the corpus in under a
second with no grid; a deliberately changed expectation fails with a
readable diff; and every semantic rule listed in
[[server-lsl-value-model]] has at least one transcript case.
