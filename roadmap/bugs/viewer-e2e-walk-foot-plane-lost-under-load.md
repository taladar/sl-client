---
id: viewer-e2e-walk-foot-plane-lost-under-load
title: Under heavy load the walking avatar reads no foot plane after a walk
topic: viewer
status: bugs
origin: commit hook run (2026-11-10)
refs: [viewer-avatar-ground-from-collision-plane]
---

## Observation

`e2e_movement`'s
`the_own_avatar_walks_and_jumps_on_a_grid_that_moves_it` failed in the
pre-commit suite and twice more run alone, each time with:

```text
the foot plane is no floor: AgentReadout { … position: Some([212.96,
128.0, 27.55]), … ground_height: Some(25.0), foot_plane: Some([0.0, 0.0,
0.0, 1.0]), feet: Some(FeetReadout { left: FootReadout { ankle: […],
ground: None }, right: FootReadout { ankle: […], ground: None },
foot_ik_weight: 0.0, standing: false }) … }
```

The load average was near 60 then (another worktree's build was running);
the failing runs took about 55 s, the passing ones 29 s. With the same
tree and the load down to 25–30 it passed twice running, and at the
previous commit it passed twice as well. The change being committed
touched only the LSL runtime and a scripted prim's move on the fake grid,
and this stage runs no script, so it is a race the load uncovers, not a
result of that change.

## Next step

Reproduce with `systemd-run --user --scope -p CPUQuota=15%` around the
test, and find which of the walk's readouts (the agent's position, the
foot raycast, the collision plane the grid sends) the check reads before
it has caught up with the walk; then wait on the readout instead of the
frame count, or fix the stale value if the viewer itself shows it.
