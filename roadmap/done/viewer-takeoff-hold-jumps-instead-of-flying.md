---
id: viewer-takeoff-hold-jumps-instead-of-flying
title: Holding the fly key jumps instead of taking off
topic: viewer
status: done
origin: observed live on OpenSim during the collision-plane ground-floor work (2026-08-12)
refs:
  - viewer-avatar-falls-through-ground
  - viewer-p31-16
---

Context: [context/viewer.md](../context/viewer.md).

Observed live (OpenSim): pressing the fly / ascend key while grounded **jumps**
rather than taking off; flight only starts on a **second** press while already
mid-jump. In real Second Life the key **jumps only on a tap** (or where flying
is disabled) and **flying starts when it is held** — so a sustained hold should
take off, not jump.

The hold-to-take-off gesture raced the jump it started. `should_take_off`
required `grounded` — the avatar standing on the ground — but the ascend key
puts `AGENT_CONTROL_UP_POS` on the wire on its very first frame, and the
simulator answers that with a hop. By the time the hold matured half a second
later the avatar was airborne, `grounded` was false, and the take-off could
never fire; the user saw a hop, and flight only arrived if a later frame
happened to catch the avatar back within the ground margin.

The reference has no such gate. `agent_jump` (`llviewerinput.cpp`) sends
`moveUp(1)` on **every** frame the key is down — the jump is expected, not a
failure mode — and once `getCurKeyElapsedTime()` passes `FLY_TIME` (0.5 s)
*and* `getCurKeyElapsedFrameCount()` passes `FLY_FRAMES` (4) it calls
`setFlying(true)` without ever asking where the avatar is. Catching a fall by
holding the key is a take-off there, and now here.

So the ground state is gone from the decision, and the frame count
[[viewer-p31-16]] always called for joins the seconds threshold: a single
stalled frame longer than `FLY_TIME` (a texture-decode hitch, a region
handover) is still a tap, not a take-off. The hold accumulator came out of the
driver as a pure step so the whole gesture — hop, mature, fly, release, tap
again — is unit-tested frame by frame.

Verified live on OpenSim: a hold hops once and then flies; a tap still only
jumps. (Underwater the hop plays its animation without leaving the ground,
which is the simulator's own behaviour, not the viewer's.)

## Stop Flying had nowhere to be pressed

Found in the same session: nothing in the UI *ended* flight but the fly toggle
key, so a fall could not be started to test catching one. The reference offers
a "Stop Flying" button — the other half of `LLPanelStandStopFlying`, whose
Stand half we already host. It floats in the reference's bottom-centre tray,
where it collides with the conversation dock; ours joins Stand Up and Stop
flycam in the bottom toolbar's reserved state slot, at the user's request.

The three are **not** mutually exclusive — the slot had been treating its
occupants as one-at-a-time, and that is wrong. Each answers an independent
state, and a state that holds is one the user may want out of whatever else
also holds: seated in the flycam shows Stand Up *and* Stop flycam, flying in
the flycam shows Stop Flying *and* Stop flycam. The flycam deliberately parks
the avatar with the fly bit set so a detached camera does not leave the body
plummeting, which is exactly why "stop the camera" and "stop the flying" have
to be offered as two separate exits rather than one hiding the other.

The one pair that cannot co-occur is Stand Up and Stop Flying — a seated avatar
is not flying — so the slot tops out at two buttons, and its reserved width
(mirrored by the trailing spacer that keeps the toolbar centred) is sized for
two side by side. Pressing Stop Flying clears the fly intent (the reference's
`gAgent.setFlying(false)`) along with the take-off hold, so a still-held ascend
key starts its half second over instead of re-launching on the next frame.

Pressing any of the three now also hands the keyboard back to the world. A click
focuses the button, a focused UI node makes the input context `UiWidget`, and
that gates the movement keys off — so Stop Flying dropped the avatar and then
swallowed the very keys that would have caught the fall, until the user clicked
the world back into focus. Both reference handlers end with `setFocus(false)`,
`onStopFlyingButtonClick` carrying the comment `EXT-482` for exactly this.
