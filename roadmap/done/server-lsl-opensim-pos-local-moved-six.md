---
id: server-lsl-opensim-pos-local-moved-six
title: OpenSim moved prim/params' PRIM_POS_LOCAL prim 6 m, where it moved it 10
topic: server
status: done
origin: server-lsl-flood-gate-passes-spaced-lines (2026-11-05)
refs: [server-lsl-lib-prim-state]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`prim/params`' `pos-local-east-15` puts the prim back with
`llSetRegionPos(home)`, sets `PRIM_POS_LOCAL` to `home + <15, 0, 0>` and reads
how far it moved. Both OpenSim engines answered 10 on 2026-10-27, which is
what OpenSim's code says (`GetSetPosTarget` caps a root prim's move at 10 m
from where it is) and what the runtime does. On 2026-11-05 they answered 6 in
four runs of five (YEngine once 10) — as if the prim stood 4 m west of `home`
when the rule list was read. The probe was re-run only to add a clock to its
`try` records; nothing between those lines changed. The replay names the line
a deviation for both OpenSim flavours.

## What to find out

Where the prim is just before the call (an `llGetPos` after
`llSetRegionPos(home)`), and whether `llSetRegionPos` has settled by then on
OpenSim: a race between the two moves would answer 6 or 10 by timing.

## Done when

The runtime answers what OpenSim answers, for the reason OpenSim does, and the
deviation goes.

## Outcome (2026-11-05)

Not the grid and not the runtime: the runner. `prim/params` now prints where
each move leaves the prim (`at`) and what each `llSetRegionPos` back answered.
On YEngine the first move put the prim `<10, 0, 19.75>` from home and the way
back answered 0: the prim stood 19.8 m under the ground (`prim/placement`'s
`ground_below` read 19.8 where it had read -6.5), so OpenSim's
`GetSetPosTarget` lifted every target onto the terrain, the way back could
not reach home, and the 10 m cap along the 15 m east, 19.75 m down diagonal
was 6.05 m east — the 6.

The prim was there because `settle_scene` anchored the rez on the first
`PRIMITIVE` the region streamed, and two link sets left in Default Region
(`Link Far …`, z 121) streamed their **children** first in most runs: a child's
position is its offset from its root, `<0, 0, 0>`, so prim A went to the
region's corner at `<0, 0, 1>`. Which came first was timing — hence 6 in four
runs of five. On OpenSim the anchor is now the test object itself, matched by
position (`OPENSIM_TEST_OBJECT`, `<120, 128, 26.25>`; a probe prim left behind
stands right above it), on the fake grid the first **root** prim of the
agent's own region; never a child or a neighbour's prim. The lsl-probes case
logs the anchor it rezzes against. With it every OpenSim flavour answers 10,
the same as aditi and the runtime, and the deviation is gone.

The race was possible from the day those link sets were made (2026-10-10). A
prim at the corner is out of chat range of the avatars at the centre, so the
`Script running` chat a new script says is missing from the transcripts it
spoiled: every OpenSim transcript of 2026-11-03 and 2026-11-05 but this one
lacked it (`data/generate-key`, `events/changed-worn`, the six `errors/flood-*`
and `errors/owner-lines` probes, `events/timer-reset`); their fault lines
name the prim at `<0,0,1>`. Those were re-recorded on both engines with the
fixed anchor on 2026-11-06. Apart from the chat, the fault position and the
clocks they read as before, save one line: YEngine's `events/changed-worn`
heard its first post-teleport `changed` at home this time, a teleport's race
rather than the placement's. `inventory/newer` lacks the chat too, but it
writes no start record on either run, so it never had a placed prim to spoil.

Re-recording `prim/params` on aditi (two runs) also moved a flood deviation:
Second Life passed `text-300`'s complaint and the projector's at the same
clock at which the run before had dropped both, so the replay now names
`text-300`'s line (the gate drops it) instead of the projector's (the gate
passes it, as aditi now did) — the window's turn, not a new rule.
