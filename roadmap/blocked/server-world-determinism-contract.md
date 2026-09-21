---
id: server-world-determinism-contract
title: Keep the fake grid reproducible once it runs scripts
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [server-world-heartbeat]
refs: [server-lsl-vm-execution, server-lsl-lib-time-timers,
  test-fake-grid-determinism]
---

Context: [context/lsl.md](../context/lsl.md).

`sl-fake-grid` is deterministic on purpose: `FakeGridBuilder::deterministic`
seeds a xorshift `IdMinter` so two grids built from one seed mint the same
uuids in the same order, and every timestamp comes from the injected
`crate::time::Now` rather than `Instant::now` — `world.rs` even freezes a
fixture creation date at a constant "because a fake grid built from a seed
mints the same run twice, and a timestamp read off the machine would be the
one field that never matched". [[test-fake-grid-determinism]] guards it.

A script engine is the largest threat that property has ever faced, and the
threat is diffuse: it is not one call to `Instant::now` but a dozen ordinary
decisions that each look local.

This task is the contract and the guard, written once so the sixteen library
tranches do not each decide for themselves:

- **The tick is the clock.** A script's notion of elapsed time is
  `ticks × step`, and `llGetTime`, `llGetAndResetTime`, `llSetTimerEvent`
  and `llSleep` are counted in ticks. Nothing in the engine reads wall time.
- **The execution budget is counted in instructions, not microseconds.** A
  wall-clock slice makes the number of instructions a script gets depend on
  the machine and on what else is running, which makes the *order* of
  observable effects machine-dependent. This is the single decision most
  likely to be made wrongly by reflex.
- **`llFrand`, `llGenerateKey` and `llListRandomize` draw from the grid's
  seeded minter**, not from `rand::thread_rng`.
- **Iteration order is defined.** Scripts run in a stated order (entity
  index, then script item id), listens fire in registration order, and the
  per-tick change set is emitted in a stable order — otherwise a `HashMap`
  walk reorders two `llSay`s between runs and a cross-check frame differs.
- **Real time is off by default.** `llHTTPRequest`, `llEmail` and anything
  else that leaves the process is disabled unless a scenario injects a
  responder ([[server-lsl-lib-http-url]]).
- **A guard test.** Extend [[test-fake-grid-determinism]]: run a scripted
  scenario twice from one seed and assert the two observable streams — every
  `ChatFromSimulator`, every object update, in order — are byte-identical.
  That test is what makes the rest of this enforceable rather than
  aspirational.

Acceptance: the contract stated in `book/src/` and in the runtime crate's
README; the guard test in place and passing over a scenario that uses a
timer, `llFrand` and two scripts talking to each other.
