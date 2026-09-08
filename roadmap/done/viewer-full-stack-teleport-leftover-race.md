---
id: viewer-full-stack-teleport-leftover-race
title: The full-stack teleport tests sampled a postcondition nothing waited for
topic: test
status: done
origin: failed a ggh pre-commit run again on 2026-09-08, after several earlier
  hits recorded on 2026-09-05
refs: [viewer-teleport-never-resets-the-world]
---

Context: [context/testing.md](../context/testing.md).

`full_stack_test`'s `a_teleport_keeps_the_subject_where_it_is` and
`a_teleport_leaks_nothing_between_regions` failed intermittently under the
pre-commit's parallel nextest run and passed alone:

```text
the region the teleport left still has objects in the world ([CircuitId(1)])
 — the scene was emptied around them rather than purged
```

It had been written off as load flakiness (and worked around by committing with
`coord.sh heavy --exclusive`), which is why it kept coming back. Both tests were
racing, and the message named the wrong mechanism.

## The race

Each test teleports, gets a frame, and immediately asserts the departed
region's objects are gone. Nothing in between waits for that:

- `teleport_to` returns on the `RegionChanged` **event**;
- `capture` waits for **quiet**, which means a region is up and no asset work is
  outstanding — it says nothing about a circuit on its way out.

The objects actually leave when the **source circuit is retired**, which the
grid does only after the destination confirms the arrival — strictly later than
both. So the tests sampled a value that had not settled, and whether it had come
down to how many frames fitted in `run_until`'s wall-clock budget. Under
parallel load, fewer did.

## The fix

`wait_for_circuits_to_leave` waits for the postcondition itself — every source
circuit gone from `ObjectState` — through the same bounded `run_until` the rest
of the harness uses. Deterministic without being weaker: a removal that never
happens still fails the test, now as a timeout carrying the harness's full
report (outstanding work, recent events, recent warnings) instead of a bare
assertion.

Verified by A/B under a 20-way CPU load — the condition that produced the
failure — with nothing but this change between the two:

| | pass | fail |
| --- | --- | --- |
| without the wait | 5 | **5** |
| with the wait | 10 | 0 |

Half the runs, not the occasional one. That is why re-running "fixed" it every
time and why it kept coming back, and it is what made committing while another
worktree agent was building a coin flip.

## What it uncovered

The failure message blamed the world-scoped purge. That purge **never runs on a
teleport at all**: `world_reset` is false because the destination is already a
child circuit by the time `TeleportFinish` arrives. Filed as
[[viewer-teleport-never-resets-the-world]] — the tests were passing for a
reason none of them stated.

## Why it was worth fixing rather than re-running

The ggh pre-commit runs the whole ~4000-test suite, so this fired on commits
that had nothing to do with teleports, and cost a full pre-commit cycle
(minutes) each time. It also trained the reflex of re-running on failure, which
is exactly the habit that lets a real regression through.
