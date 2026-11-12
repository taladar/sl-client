---
id: server-fake-grid-prim-sit-states-no-sit-animation
title: A sit on a prim states no sit animation, to the sitter or anyone else
topic: server
status: bugs
origin: server-fake-grid-scripted-avatars (2026-11-12)
refs: [server-fake-grid-scripted-avatars, server-world-sit-and-attach]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

A viewer standing beside the scripted avatar of the `scripted-avatar` scene
(`tests/scripted_avatars.rs`) was told its walk and stand animations and then
its seat (its body parented to the pose ball), but no `AvatarAnimation`
naming a sit animation between the two. The fake grid's seat path
(`seating.rs`, `FakeAgent::seat_on`) changes the sitter's parent and halts
its movement (`movement::advance`), and leaves its animation set at the
stand it had: no posture for a prim sit exists in `movement::Posture`, only
`SittingOnGround`.

## The reference

OpenSim's `ScenePresence` sets the movement animation `"SIT"` on a sit
(`HandleAgentSit`; `"SIT_GROUND"` for a sit on the ground) and back to
`"STAND"` on a stand-up (`StandUp`), and so tells every viewer in the region
through its animator. Second Life is expected to do the same (a seated
avatar plays `sit` until a script stops it — the pose ball's
`llStopAnimation("sit")` assumes so); measure which animation and when on
aditi before modelling it.

## Wanted

The sitter's animation set gains the grid's sit animation when it is seated
on a prim and loses it when it stands, stated to its own viewer and to the
region's others; a script's `llStopAnimation("sit")` stops it.
