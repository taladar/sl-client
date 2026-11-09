---
id: test-lsl-differential-corpus-triage
title: Triage every probe's differences between each grid and its fake flavour
topic: test
status: done
origin: test-lsl-differential-opensim (2026-11-07)
refs: [test-lsl-differential-opensim, server-lsl-event-raise-timing,
  server-lsl-sl-placement-lag]
---

Context: [context/lsl.md](../context/lsl.md).

[[test-lsl-differential-opensim]] ran the whole probe corpus on both fake
flavours (`probes/results/fake-sl/`, `probes/results/fake-opensim/`) and
cleared the **differential suite** (`sl-conformance differential`'s default
probes): every difference it shows is fixed or listed in
`sl-lsl-runtime/probes/accepted.toml`. The rest of the corpus was compared
but not triaged. `sl-conformance compare-probes --grid aditi --grid fake-sl`
and `--grid opensim-yengine --grid fake-opensim` list it; on 2026-11-07, at
the commit that closed the differential task, 57 probes agreed between aditi
and fake-sl and 58 between the local OpenSim and fake-opensim.

Differing between aditi and fake-sl (differences in brackets):
data/linkset-data (120), detection/cast-ray (85), errors/flood (12),
errors/flood-repeat (1), errors/math (10), errors/math-lso (20),
errors/owner-lines (2), errors/runtime-errors (12), events/changed-contents
(37), events/changed-edits (72), events/changed-owner (4),
events/changed-worn (5), events/collision (4), events/merging (5),
events/other-avatar (1), events/state-drops (7), experience/no-experience
(8), experience/powers (8), http/options (13), http/outbound (11), http/url
(19), inventory/give (4), inventory/listing (17), inventory/newer (1),
inventory/rez (29), land/day-cycle (1), land/env (10), land/ground (19),
land/parcel (25), land/region (8), land/sl-only (64), math/rotation-bits
(17), memory/budget (161), memory/budget-lso (239), memory/collision (96),
memory/object (10), memory/recursion (147), memory/sizes (97), money/lapse
(3), money/pay (2), money/pay-price (6), physics/character (19),
physics/forces (25), physics/keyframed (48), physics/material (29),
physics/status (42), physics/targets (15), physics/vehicles (39),
prim/changed (34), prim/effects (4), prim/identity (2), prim/link (85),
prim/link-refusals (39), prim/params (2), prim/params-rules (12),
throttle/cast-ray (1), throttle/email (4), time/clocks (1),
time/reset-mid-sleep (1), time/sleep-while-stopped (2), vm/region-budget
(2); vm/throughput has no fake-sl result (the 3-hour corpus run timed out
in it, its last probe).

Differing between the local OpenSim (YEngine) and fake-opensim:
comms/link-messages (4), comms/owner-away (1), data/json (5),
detection/cast-ray (126), errors/bounds (3), errors/bounds-newer (19),
errors/flood (19), errors/flood-repeat (1), errors/math (6),
errors/math-lso (4), errors/owner-lines (2), errors/runtime-errors (4),
events/changed-contents (38), events/changed-edits (78),
events/changed-owner (6), events/changed-worn (7), events/collision (4),
events/detected-cleared (6), events/state-drops (7), events/state-self (1),
experience/powers (8), http/outbound (2), http/url (4), inventory/give (4),
inventory/listing (16), inventory/rez (29), land/env (5), land/ground (12),
land/parcel (13), land/region (2), math/float-rounding (22),
math/rotation-bits (405), memory/collision (28), memory/object (7),
money/lapse (1), money/pay-price (4), ossl/available (31), ossl/errors
(10), persistence/lists (1), persistence/restart (6), physics/forces (25),
physics/keyframed (46), physics/material (29), physics/status (42),
physics/targets (15), physics/vehicles (39), prim/changed (34),
prim/identity (2), prim/link (102), prim/link-refusals (54), prim/params
(4), prim/params-rules (8), strings/conversions (36), strings/newer (52),
throttle/cast-ray (1), throttle/email (3), time/clocks (1),
time/reset-mid-sleep (1), time/sleep-while-stopped (2), vm/call-cost-newer
(27).

Much of it is expected and only wants an entry: the region's other
residents' objects (detection, prim/link), the fake grid's own terrain and
build height (land/ground), physics the fake grid does not run (the
`physics/*` probes, whole-probe entries until the physics tasks land),
OpenSim's doubles (math/float-rounding, math/rotation-bits,
strings/conversions), and Second Life's memory accounting, which the
runtime models in its own units (memory/*). The rest are findings.

Wanted, probe by probe: each difference fixed (with a test holding the fix)
or recorded in `accepted.toml` — deliberate with its reason, or a known
defect with the task that removes it (a new bug file where none exists);
whole-probe entries for probes the fake grid cannot run yet. Re-run each
fake flavour's corpus first (`sl-conformance run --grid fake-sl --timeout
14400 lsl-probes`; Second Life's frame makes its run longer than three
hours), since the results above came from a binary before the event-queue
batching fix.

Acceptance: `compare-probes` between each grid and its fake flavour reports
every probe as agreeing, or agreeing but for accepted differences.

## Outcome (2026-11-09)

Both fake corpora were re-run with the fixed binary and every probe now
agrees with its grid, or agrees but for entries in `accepted.toml`; the
conformance test holding the entries now names every stale one in a single
run. Fixed on the way, each with a test: the runner's settle after each
upload and its clearing of the other avatar's active group (so
`llDetectedGroup` reads alike on every grid) and a name scrubber that keeps a
test account's name where it is also a word the probes write; teleport
answers from a region demoted to a child agent; script counts and memory in
object details; `OBJECT_REZZER_KEY`, `llGetLinkKey(0)` from a child,
`llGetVel`, item names and undescribed takes, OpenSim's permission masks,
rotation of four zeros, a state change inside a function, detected values
past the end, the newer functions OpenSim does not compile, a region-wide
revoke, a fault's lines against the flood limit, and the order of link
numbers and linkset-data events.

Left as known defects, each with a new task and the entries that name it:
[[server-lsl-mono-collision-model-under-lowered-limit]],
[[server-lsl-opensim-grow-loop-slow]],
[[server-lsl-scripted-teleport-unmodelled]],
[[server-lsl-link-applies-after-tick]],
[[server-fake-grid-worn-departure-change]],
[[server-lsl-restarted-sleeper-order]],
[[server-lsl-opensim-child-move-change-late]], and the idea
[[server-lsl-cast-ray-cost]].
