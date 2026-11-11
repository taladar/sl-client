---
id: server-lsl-mono-collision-model-under-lowered-limit
title: A script 1 KB over its use collides in the runtime where Mono carries on
topic: server
status: done
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [test-lsl-differential-corpus-triage, server-lsl-mono-garbage-room]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`memory/budget` sets `llSetMemoryLimit(llGetUsedMemory() + 1024)` and goes on
calling a helper that builds a line of about 90 characters. aditi carries on
with 812 to 962 bytes free, its use rising some 150 to 210 bytes while the line
is built. The runtime's Second Life flavour raises a stack-heap collision in
the next call, so on fake-sl the probe never prints its last six `set-limit`
lines or its `end`.

## Cause (as first guessed, wrong)

The collision model (`sl-lsl-runtime/src/vm/memory.rs`: `MONO_FRAME`,
`slot_size`, and the locals' heap objects through `local_cost`) charges the
call chain `state_entry → ask_limit → read → emit` and its temporaries more
than 1 KB. `memory/recursion`'s Limited script shows the same lean: it reaches
depth 73 where aditi reaches 129 to 253.

## Cause (found, 2026-11-11)

The chain held 128 bytes of stack at the collision, not 1 KB. The probe goes
on to ask for `used`, the use it read in `state_entry`, from `ask_limit`. The
runtime's use had not moved since, so the limit was taken at exactly the
heap, and the next call collided. aditi refused that ask, its use having risen
above the stale read.

Two aditi measurements settled what to model:

- `memory/budget` gained a last ask, `llSetMemoryLimit(llGetUsedMemory())`
  in one expression, the limit put back at once: aditi **took** it. The
  heap-only check was right.
- A new probe, `memory/stack`, split a recursion's depth, five runs:
  Quiet (a library call every sixteenth level) 2 256 to 2 464; Quiet limited
  (the same under `used + 4096`) 560 to 928; Heavy (over 32 KB of global
  list) 912 to 1 712; Litter (a library call and a dropped 104-character
  string at every level) 1 040 to 1 280. Heavy first made its list one
  integer at a time, which the runtime's list-join speed (fitted to
  `r = L + [-1]`) took past the controller's ten quiet seconds, so it makes
  it ten at a time since the fourth run.

So the depth is not set by garbage: Litter drops ten times Narrow's garbage
and reaches Narrow's depth. What Narrow and Litter share, and Quiet lacks, is
a library call at every level. And aditi's own Narrow transcript reads 30 and
a quarter bytes more `used` a level, which the old model said never happens.

## Done

`vm/memory.rs`'s Mono stack is now two parts:

- A frame is `MONO_FRAME` (20) and its slots (`slot_size`), never read by
  `llGetUsedMemory`. Frames, heap and locals meet in the script's 64 KB
  whatever its limit; against a lowered limit only the slots count.
- A frame's first library call gives it a call context, `MONO_CALL_CONTEXT`
  (30), held on the heap until it returns. `llGetUsedMemory` reads it, and so
  does `llSetMemoryLimit`'s check. In `ask_limit` it pushes the use past the
  stale read, so the ask is refused there as on aditi.

A store that makes nothing bigger no longer checks for a collision. That lets
`set-limit-now` store its integers under a limit of exactly the use, as aditi
did. `MONO_SCRIPT_CODE` drops by the 60 bytes of the two call contexts each
first reading now carries. Here the probes reach Narrow 1 092, Wide 424, Quiet
2 272, Heavy 1 024 and Litter 1 072: inside aditi's ranges. `memory/budget`
runs to its end with every `set-limit` answer aditi's. Full list now
collides at aditi's 3 660 on fake-sl, and its accepted entry is gone.

Under a lowered limit Quiet limited reaches 672, inside aditi's 560 to 928.
Limited, a library call at every level, reaches 118 against 129 to 253, and
the Limited list 500 against 540: some 600 to 1 000 bytes less room than
aditi's. That is likely the garbage aditi's reading carried when the limit
was set, collected later, and it is [[server-lsl-mono-garbage-room]]'s.

Tests (`vm/tests/memory.rs`): `the_stack_meets_the_heap_where_aditi_said`
(the four `memory/stack` scripts), and
`a_limit_asked_for_what_the_script_used_is_refused_after_a_call` (the
budget probe end to end against aditi's answers). The recursion test also
holds Narrow's use to rising one call context a level, within a byte a level
of aditi's. The harness's `print` rewrite keeps a library call, as the call
it stands for does. Book: `gridspec/lsl.md` § Memory and limits,
`simulator/lsl-engine.md` § As built: memory.
