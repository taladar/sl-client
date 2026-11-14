---
id: viewer-e2e-walk-foot-plane-lost-under-load
title: Under heavy load the walking avatar reads no foot plane after a walk
topic: viewer
status: done
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

## Resolved (2026-11-11): already fixed by the three-frame jump tap

The failing hook ran on master at `776d3119`, which did not yet hold
`cc97b48d` (made on `viewer-features` on 2026-11-09, merged into master
with `30d3a00b` on 2026-11-11). Master's stage still tapped the jump key
for six frames and had no "back down" check. On a loaded machine six
frames outlast both `AutomaticFly` thresholds (half a second and more than
four frames), so the tap was a take-off: the avatar hovered about 1.6 m up
(`z` 27.55 over ground at 25.95), the next walk flew it forward at the
Second Life flavour's 16 m/s to `x` 212.96 (a walk reaches about 150), and
the foot-plane wait ran out its 30 s with the in-air plane `[0, 0, 0, 1]`.
That is the whole readout and the 55 s against 29 s.

The three-frame tap never passes the frame bar, and the stage now asserts
the avatar is back down before it walks again. On the current tree the
stage passed three runs at a 15% CPU quota, eight on two or four CPUs
shared with ten or twelve busy loops, and in a full `sl-client-bevy-viewer`
suite run (612 tests).

The movement driver now logs at info level whenever flight starts or ends
(the fly key, a held ascend's take-off, a landing), so a test log shows a
take-off like this one directly.
