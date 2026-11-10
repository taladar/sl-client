---
id: server-lsl-opensim-child-move-change-late
title: OpenSim's position change in a moved child comes after the mover's handler
topic: server
status: done
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

## Resolution (2026-11-10)

OpenSim raises the change inside the part's offset setter
(`SceneObjectPart.OffsetPosition`), unconditionally, and YEngine runs it on
another thread while the mover goes on. Two things followed:

- **The count.** The fake grid raised nothing for a child moved to where it
  already was; OpenSim raises one (the group's setter for a root compares,
  the part's for a child does not). `move_to` and the fake host's
  `set_position` now raise for every child move on OpenSim: fourteen, as the
  local OpenSim printed.
- **The timing.** A setter that raises `changed` in another script on
  OpenSim now asks the VM to step aside (`Request::Pass`,
  `Outcome::Passed`); the engine posts the events, runs the turns of the
  scripts they reached and resumes the caller, so the child hears each move
  as it lands. The move just before `break-2` is heard with the child still
  linked (`link=2 prims=2`), as on OpenSim, where it read `link=0` after the
  break before.

A second YEngine run (2026-11-10, committed) put the child's lines among the
Leader's quite differently from the first, and read the change at the link
with the linkset already numbered, so the interleave is thread timing. The
task's accepted entry is replaced by two deliberate ones saying so (the
child's lines' places, and the `break-2` try line the alignment pairs
around them). `a_moved_child_prims_script_hears_each_move_as_it_lands`
(`vm/tests/linking.rs`) holds the order per flavour.
