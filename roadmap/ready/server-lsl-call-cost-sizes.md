---
id: server-lsl-call-cost-sizes
title: Measure size-proportional call costs and the region's script budget on aditi
topic: server
status: ready
origin: server-lsl-vm-execution review (2026-09-28)
points: 3
refs: [server-lsl-vm-execution, server-lsl-lib-strings-lists,
  server-lsl-memory-sizes]
---

Context: [context/lsl.md](../context/lsl.md).

The VM charges every library call one flat cost (`BUILTIN_CALL_COST`, 626
instructions) and every loop back-edge 513, both fitted to loops timed on aditi
([[server-lsl-vm-execution]]). Two things the book's scheduling section asks
for were left open there, because nothing measured them:

- **Size-proportional charges.** A call whose work grows with its arguments —
  `llListSort`, `llList2CSV`, `llDumpList2String`, `llParseString2List`,
  `llSubStringIndex`, `llReplaceSubString`, `llListFindList` — should cost more
  for a longer list or string, or a loop over a big list runs faster on the
  fake grid than on Second Life and timing-sensitive content behaves
  differently. The charge must be measured, not guessed: time each function
  over a million iterations (a shorter run is wrong by a whole frame, since
  `llGetTime` moves in 1/45 s steps) at three or four argument sizes, fit a
  per-element / per-character slope over the flat cost, and put it in the
  descriptor or beside the implementation. Do it when the functions exist —
  [[server-lsl-lib-strings-lists]] is where most of them land — and check
  whether `llGetListLength`, already implemented, is flat as expected.
- **The region's budget.** `REGION_SCRIPT_SHARES` (16 per-script shares a
  tick) is a stated decision: aditi only showed that two busy scripts in one
  prim each keep the one-script rate. Rez more busy scripts (8, 16, 32, 64)
  and watch the per-script rate and the region's *scripts run* percentage in
  the statistics bar; the count where the rate starts to fall is the share
  count to copy.

The probe to start from is the throughput script of the VM task: a loop of a
fixed count, timed with `llResetTime` / `llGetTime`, reported with
`llOwnerSay`, after one warm-up run (the first run after a save is slower
while Mono compiles).

Acceptance: each size-dependent function implemented by then has a charge that
reproduces its aditi rate at every measured size within a few per cent, and
`REGION_SCRIPT_SHARES` is either the measured count or documented as still a
decision with the measurement that failed to find one.
