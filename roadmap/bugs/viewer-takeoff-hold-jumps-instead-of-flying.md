---
id: viewer-takeoff-hold-jumps-instead-of-flying
title: Holding the fly key jumps instead of taking off
topic: viewer
status: bugs
origin: observed live on OpenSim during the collision-plane ground-floor work (2026-08-12)
refs:
  - viewer-avatar-falls-through-ground
---

Context: [context/viewer.md](../context/viewer.md).

Observed live (OpenSim): pressing the fly / ascend key while grounded **jumps**
rather than taking off; flight only starts on a **second** press while already
mid-jump. In real Second Life the key **jumps only on a tap** (or where flying
is disabled) and **flying starts when it is held** — so a sustained hold should
take off, not jump.

Almost certainly **pre-existing**, not caused by the collision-plane
ground-floor change that was in flight when this was noticed: take-off is
decided by `should_take_off` in `movement.rs`, which reads
`grounded = AvatarMotion::at_ground_floor(...)` (the raw
`AvatarMotion::position` + terrain land height). That change touches neither —
and if anything the new land cache makes `grounded` *more* reliably true (fewer
`land = None` frames), which would help take-off, not break it.

## Where to look (`movement.rs`)

- `should_take_off(flying, grounded, ascend_held_secs, can_fly)` requires
  `ascend_held_secs >= TAKE_OFF_HOLD_SECS` — check that a held ascend key
  actually **accumulates** `ascend_held_secs` (a tap that fires a jump impulse
  may be resetting or not advancing the hold timer, so the threshold is never
  reached and the avatar keeps jumping).
- Whether the jump impulse (`AGENT_CONTROL_UP_POS`) is sent **every** frame the
  key is down (so the sim keeps jumping) instead of the client withholding it
  once it commits to a take-off after the hold threshold.
- `can_fly` / the parcel's fly-allowed flag: confirm flight is actually
  permitted where it was tried (a no-fly parcel would *correctly* jump).

## To confirm on the next repro

- With `SL_VIEWER_LOG_LOCOMOTION=1` (and/or logging `ascend_held_secs`,
  `grounded`, `should_take_off`): does the hold timer climb past
  `TAKE_OFF_HOLD_SECS` while the key is held, or reset each frame?
- Does a **tap** correctly jump and a **hold** correctly fly once the timer
  logic is right? Compare against the reference viewer's tap-vs-hold behaviour.
