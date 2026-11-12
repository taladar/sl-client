---
id: server-fake-grid-prim-sit-states-no-sit-animation
title: A sit on a prim states no sit animation, to the sitter or anyone else
topic: server
status: done
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

## Done (2026-11-12)

Measured first: a new probe, `avatar/sit-animation`, sits the owner on a
prim and has the seat's script read `llGetAnimation`, `llGetAgentInfo` and
whether `sit` is in `llGetAnimationList`, stop `sit`, start it again, swap
it for `dance1` as a pose ball does, and read again after a stand. Second
Life (aditi, Mauve) and OpenSim (both engines) agree on every record: the
sitter plays the built-in `sit` once seated; a script's
`llStopAnimation("sit")` leaves nothing in its place (OpenSim's
`RemoveAnimation(id, true)` drops the default); a start plays it again as
the seat's own; and a stand brings `stand` back without stopping the seat's
pose. Two of the three aditi runs showed no built-in `sit` while seated,
the account's animation overrider playing its own asset for `Sitting`; the
committed one is such a run, so `probes/accepted.toml` records that one
field against `fake-sl` as the overrider's.

The fake grid now:

- has a `SittingOnPrim` posture (`Sitting`, the built-in `sit`), entered and
  stated **with the seat** (the sit request, or a script's `llSitOnLink`),
  not at the next movement tick. A tick later was too late: the seat's
  script hears `changed` in the next script tick, and its stop found `sit`
  not yet playing, so the late `sit` went in under the pose. A stand
  (`StoodUp`, or the region's `Unseated`) puts it back to standing at once;
- lets a script stop the default animation (`AnimationSet::apply_scripted`),
  which a viewer's `AgentAnimation` still cannot, the set then stating no
  default until the posture changes;
- plays a script's override of whichever state the posture is in
  (`llSetAnimationOverride("Sitting", …)` and the rest), where before only a
  `Standing` override was applied, and only until the avatar next moved.

`tests/scripted_avatars.rs`' `a_sitter_plays_sit_until_its_seat_poses_it`
holds the sequence another viewer is told on both flavours (`sit`, nothing,
the pose, `stand` under the pose); it failed with `sit` stated a tick late.
Both fake flavours' `avatar/sit-animation` runs match their grids.

Left: the fake grid names the avatar, not the seat, as the source of what a
script starts (`server-fake-grid-script-animation-source-is-the-avatar`).
The scene's sitter still shows upright in a capture, and that is correct:
the fixture pose ball stops `sit` and plays a chest twist, and the reference
viewer draws a seated avatar with no sit animation standing (it has no sit
fallback of its own).
