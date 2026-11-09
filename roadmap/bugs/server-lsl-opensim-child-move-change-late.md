---
id: server-lsl-opensim-child-move-change-late
title: OpenSim's position change in a moved child comes after the mover's handler
topic: server
status: bugs
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [test-lsl-differential-corpus-triage]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

In `prim/link`'s move steps the Leader moves its child again and again
(`pos-local-5` to `position-60`, then `child-setpos`). On the local OpenSim
(YEngine) the child's `changed(0x8000)` lines arrive between the Leader's
lines, one or two after each move. On fake-opensim all of them arrive in one
run after the Leader's last move, and there are 16 against OpenSim's 17.
Second Life raises no position change, so aditi and fake-sl agree.

## Fix

Raise OpenSim's position change in the moved prim's scripts as each move
lands, rather than after the moving script's handler, and match OpenSim's
count per move (a move by a rule list and the update after it may each raise
one). Then drop the entries in `probes/accepted.toml` that name this task.
