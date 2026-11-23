---
id: viewer-teleport-back-gets-no-answer
title: A teleport back to a recently left region can get no answer from the grid
topic: viewer
status: bugs
origin: viewer-own-avatar-broken-after-teleport live runs on aditi (2026-11-23)
refs: [viewer-own-avatar-broken-after-teleport]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Teleporting by the world map between Mauve and Ahern on aditi, two of the five
runs that made more than one teleport stalled on the same step: the third
teleport, from Ahern back to Mauve. The viewer logged `teleport timed out: the
grid neither carried it out nor refused it`. In the run that retried, the second
and third requests timed out the same way, so the session did not recover by
asking again.

| Time spent in each region | Runs | Result |
| --- | --- | --- |
| 60 s | 2 | both stalled on the third teleport |
| 30 s | 3 | 34 teleports, none stalled |

Two runs are too few to say the time spent in a region is the cause.

## To find out

- What the viewer sent for the stalled request, and whether the simulator
  answered anything (`TeleportStart`, `TeleportProgress`, `TeleportFailed`),
  with a wire log of the stalled step.
- Whether Firestorm can make the same trip at the same pace.
- Whether a teleport to a third region still works from the stalled state.
