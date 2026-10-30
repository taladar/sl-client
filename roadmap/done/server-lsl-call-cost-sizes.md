---
id: server-lsl-call-cost-sizes
title: Measure size-proportional call costs and the region's script budget on aditi
topic: server
status: done
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

## From gridspec-lsl-probe-corpus (2026-10-23)

The throughput probe is committed (`sl-lsl-runtime/probes/vm/throughput.lsl`)
and its aditi run is a result file. On 2026-10-23 it measured 511 000/s
empty, 500 000/s with three operations and 228 000/s calling `llAbs`, the
rates the flat costs were fitted to. A size probe belongs beside it in
`probes/vm/`, run with `sl-conformance run --grid aditi lsl-probes`.

## Measured (gridspec-lsl-strings-math, 2026-10-27)

The size probes are committed: `sl-lsl-runtime/probes/vm/call-cost-lists`,
`call-cost-strings` and `call-cost-newer` time 26 functions at four sizes
each on aditi (lists and strings twice, the second run committed) and both
OpenSim engines; the per-call cost above an
empty loop is tabled in `book/src/gridspec/lsl.md` § *What a call costs*.
`llGetListLength` and `llList2Integer` are flat; on Mono
`llStringLength`, `llGetSubString` and `llOrd` grow with the string (about
0.65 µs a character). Fit the
slopes from those transcripts. The region's budget half is still open.

## Done (2026-10-29)

- **Size-proportional charges.** `sl-lsl-runtime/src/vm/cost.rs`: a
  `SizeCost` is a cost's measured points joined by straight lines, flat
  before the first and on along the last slope after it, converted to
  instructions at `SCRIPT_INSTRUCTIONS_PER_SECOND`. Of the measured
  operations only `+` exists yet: `list + list` and `string + string` are
  charged by the length of the result (`LIST_JOIN`, `STRING_JOIN`, fitted
  from the committed aditi transcripts). `llGetListLength` is flat on aditi
  (2.43–2.50 µs at every size) and the flat call cost charges it 2.42 µs.
  `joins_and_flat_calls_cost_what_they_cost_on_aditi` runs the probes' own
  loops in the VM at all four sizes and holds each charge to its transcript
  within 5 %. The functions themselves are still unwritten; their charges
  go with them ([[server-lsl-lib-strings-lists]] says how).
- **Overdraft.** A list join of 500 elements costs two million
  instructions, more than a tick's share, and the engine used to let a
  slice overrun its share unpaid — so a script doing big joins ran faster
  than one doing small ones. What a slice overdraws now comes out of the
  instance's next shares and the region's budget.
- **The region's budget.** New probe `vm/region-budget` (a leader and a
  `Worker` object it rezzes 1, 16, 64, 128 and 256 copies of) on aditi: 128
  busy scripts each keep the one-script rate, 256 run at half, the total
  flat at 61.9 M iterations a second — 130 shares (134 in an earlier run).
  `REGION_SCRIPT_SHARES` is 130, and `EngineConfig::region_budget` is a
  `u64`, since 130 shares of a step longer than about 0.13 s no longer fit
  a `u32`. Script time reaches 23.9 ms, a frame at 45 fps; `llGetTime`
  keeps pace with real time while the region's time dilation dips to 0.77.
  Book: gridspec `lsl.md` § *How many scripts a region runs*.
- **Runner.** The probe header's `stats` takes a list (`active-scripts`,
  `scripts-run`, `script-time`, `time-dilation`), and a new `deadline`
  key lets a probe run past three minutes (`vm/region-budget` asks for
  five). Under XEngine the 256 stage still does not finish in five
  minutes; its transcript is committed as incomplete.
