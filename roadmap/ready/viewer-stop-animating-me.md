---
id: viewer-stop-animating-me
title: Stop animating me — stop every own animation, reset the overrides, revoke animation permissions
topic: viewer
status: ready
origin: gridspec-animations (2026-10-19)
refs: [gridspec-animations, viewer-animation-overrider]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's *Stop animating me* (`LLAgent::stopCurrentAnimations`) is the
only way out of an animation nobody will stop: both grids keep a script's
animation playing after its object is deleted, and on Second Life an
`llSetAnimationOverride` posture stays until it is reset
(`book/src/gridspec/avatars.md` § Animations, measured by `animations`). Our
viewer has no such action.

What the reference does, in order:

1. stops every animation in the own avatar's signalled set except
   `do_not_disturb` and `sit_ground_constrained`, in one `AgentAnimation`
   (`sendAnimationRequests`, skipped when the list is empty);
2. sends the reset: one `AgentAnimation` block stopping the **nil** id
   (`sendAnimationStateReset`) — measured to clear Second Life's posture
   override and to change nothing on OpenSim;
3. when `RevokePermsOnStopAnimation` is set, revokes `TRIGGER_ANIMATION` and
   `OVERRIDE_ANIMATIONS` region-wide (`RevokePermissions` with the region id,
   `Session::revoke_script_permissions`), and stands the avatar up if it sat,
   since the sit's auto-granted permission is gone.

Wire it as a menu action (the reference has it in the avatar's own pie and
under World/Avatar), with a preference for the revoke. Hold it with an e2e
test against both fake flavours (the commands the grid hears) and a unit test
of which animations are stopped.
