---
id: test-lsl-content-suite
title: A test harness for somebody else's LSL content
topic: test
status: blocked
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
