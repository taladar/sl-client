---
id: test-lsl-content-suite
title: A test harness for somebody else's LSL content
topic: test
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20); use case raised by the user
points: 8
blocked_by: [server-fake-grid-scripted-avatars]
refs: [server-fake-grid-scripted-scenario, test-lsl-script-corpus,
  test-lsl-differential-opensim, repl-lsl-script-control]
---

Context: [context/lsl.md](../context/lsl.md).

Everything else in this topic treats the script engine as something to
*build*. This task treats it as something to **ship**: an offline,
deterministic, in-process grid that runs a given set of LSL scripts and
asserts what they do is a testing tool for Second Life content, and
there is essentially nothing like it. Today the only way to regression-
test a scripted product is to rez it on a live region and click it.

The distinction from [[test-lsl-script-corpus]] matters. That corpus is
*ours*: small scripts, a mock host, pinning the runtime's own
semantics. This is a harness for scripts **we did not write** — a
vendor, a HUD, an animation system, a whole linkset of them — where the
scripts are the thing under test and the grid is the fixture.

What it needs beyond what the rest of the topic already builds:

- **A declarative case format.** A case names: the objects to rez and
  what goes in each one's inventory (scripts, notecards, animations,
  sounds, textures), the avatars to bring and their scripted behaviour
  ([[server-fake-grid-scripted-avatars]]), the stimulus sequence, and
  the expectations — what was said, on which channel, by which object;
  what moved; which dialog appeared with which buttons; what was paid;
  what ended up in whose inventory. Not a Rust test: a data file, so a
  content creator who does not write Rust can use it.
- **A runner** — `cargo run -p <harness> -- cases/` or a library API —
  that starts a grid per case, runs it for a bounded number of ticks,
  and reports pass/fail with a diff of the observable transcript. Exit
  status meaningful, output greppable, no window, no network. That is a
  CI job.
- **A real asset story.** Content under test refers to animations,
  sounds and textures by uuid. The harness has to let a case supply them
  — the crate already serves a grid-wide asset store and has
  `sl-test-assets` for synthetic ones — and to answer honestly for a
  uuid it was not given rather than silently returning nothing.
- **Failure that is legible.** "Expected the vendor to say `Thank you`
  on channel 0 within 5 s; it said nothing. Script `vendor` stopped at
  line 47 with a Math Error." The transcript and the run-time error
  reporting ([[server-lsl-runtime-errors]]) are what make that possible,
  which is another reason the error path is not optional.
- **An honest statement of fidelity.** This grid is not Second Life, and
  a case that passes here can still fail there; the differential runner
  ([[test-lsl-differential-opensim]]) is what bounds the gap, and the
  harness should say which surfaces are known-approximate (physics,
  vehicles, timing under load) rather than implying parity.

Worth noting it changes the priority order of the library tranches: what
real content uses most — `llListen`, `llDialog`, `llSetLinkPrimitiveParams`,
`llGiveInventory`, `llJson*`, `llHTTPRequest`, `llSetTimerEvent`,
`llMessageLinked` — matters more than completeness for its own sake. If
this use case is taken seriously, order the tranches by what a survey of
real scripts actually calls.

Acceptance: a non-trivial third-party-shaped linkset (a vendor with a
dialog menu, a notecard config and a payment path) tested end to end
from a data file, passing and then failing informatively when one of its
scripts is broken; the whole run in one command with no grid, no window
and no network.

## Done (2026-11-12)

A new crate, `sl-content-test` (library and binary), with the book chapter
`tools/content-test.md`. `cargo run --release -p sl-content-test --
sl-content-test/cases` plays every `case.toml` / `*.case.toml` below the
paths given on an in-process fake grid, one run per grid the case names, and
prints one `PASS` / `FAIL` line per run with what went wrong indented under
it; the exit status is 0, 1 (a case failed) or 2 (a case could not be read
or played).

- **The case format** (`case.rs`) is TOML with every field checked
  (`deny_unknown_fields`): objects with linked prims, each holding scripts,
  notecards, object items and animation, sound and texture assets read from
  files beside the case; `[[asset]]` for keys a script names alone; owners;
  scripted avatars with an answer policy and steps (walk, touch, sit, stand,
  say, pay, answer a dialog or a question, await a line); and four kinds of
  expectation: a line said (exact or containing, by whom, on which channel,
  or among what one avatar was told, or `never`), an avatar's transcript
  holding lines in order, an expected run-time error, and where a prim
  stands at the end.
- **The runner** (`run.rs`) builds the region from the case with minted,
  seeded ids, steps its heartbeat by hand until every avatar has finished
  and `settle_ticks` more have passed (bounded by `max_ticks`), and collects
  the region's chat, the run-time errors, the stubbed calls, each avatar's
  transcript and heard lines, and every prim's final place; it compiles each
  script as the imitated grid does and lists the keys scripts write that the
  case does not supply.
- **The verdict** (`verdict.rs`) fails a compile error, any run-time error
  no expectation names (with the script, line, column and event), an avatar
  that did not finish (with the step it is stuck on and its transcript's
  tail), and each unmet expectation (with what that speaker did say, or the
  transcript); notes, not failures, for an unsupplied key and a stubbed
  library call.
- **The grid** gained what the harness reads: `FakeGrid::overhear` (a
  lossless tap on a region's listens that hears every line on every
  channel), `FakeGrid::script_faults` (each fault with its source position,
  handed over by a new `Host::faulted`, which the runtime calls whatever
  Second Life's flood limit lets through), `FakeGrid::stubbed_calls`,
  `FakeGrid::with_region_world`; scripted avatars now keep `heard()` (every
  line and instant message their client was told) and `step()`, and note an
  object's inventory offer in their transcript (`Vendor offered Blue
  gadget`).
- **The acceptance case** is `sl-content-test/cases/vendor`: a two-prim
  linkset whose root reads its price and colours from a notecard, menus the
  toucher, tells the display child the choice by link message, and on
  payment thanks the buyer and offers the gadget. `tests/vendor.rs` plays it
  passing, then with its vendor script dividing by zero (the report names
  `script "vendor" in "Gadget Vendor" stopped at line 102, column 27 in
  money with Math Error`, the unmet thanks and offer, and the buyer stuck on
  step 6 of 6), then with a script that does not compile.

Differences from the task as written:

- **What ended up in whose inventory** is checked as far as the grid goes:
  an object's offer is in the avatar's transcript, but nothing is filed,
  since filing a task-inventory gift on the accept (Second Life) or at the
  offer (OpenSim) is `test-fake-grid-lsl-offline-cases`'s.
- **The tranche order** the task suggests (by what a survey of real scripts
  calls) is not this task's to change and was not surveyed.
- **OpenSim payments**: OpenSim tells a payer nothing of a payment to an
  object, so a scripted avatar's `pay` step never completes on an `opensim`
  run; the vendor case runs on `second-life` alone and the book says so.
