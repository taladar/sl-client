---
id: viewer-own-avatar-facing-drifts-idle
title: Own avatar forward direction drifts every few seconds while idle
topic: viewer
status: done
origin: observed live on aditi during the collision-plane ground-floor work (2026-08-12)
refs:
  - viewer-avatar-falls-through-ground
---

Context: [context/viewer.md](../context/viewer.md).

Observed live on aditi: the **own** avatar's **forward/facing direction changes
slightly every few seconds** without any turn key pressed (the avatar was not
being actively steered). Small, periodic yaw drift.

Almost certainly **not** related to the collision-plane ground-floor change that
was in flight when this was noticed — that edit only touches the *vertical*
(`AvatarMotion` `position.z`, the anchor's target/rendered `y`); the avatar's
heading comes from `AvatarMotion::rotation` eased through
`smoothed_rotation` (`physics.rs`), which it does not touch.

## Likely suspects (to check)

- The simulator re-broadcasting a slightly jittery `ObjectUpdate` rotation for
  the own avatar (terse-update quantised rotation echoes), which the P31.7
  rotation ease then glides toward — a periodic micro-yaw as the coarse
  quantised value flips between adjacent codes.
- The movement/heading seed (`movement.rs`) re-seeding the walk heading from the
  reported yaw when idle.
- A look-at / camera-driven `SetRotation` being sent and echoed back.

## To pin down on the next repro

- Is it the **rendered** rotation only, or does the authoritative
  `AvatarMotion::rotation` itself change (log both)?
- Does it correlate with `ObjectUpdate` arrivals (every few seconds ≈ the terse
  update cadence for a still avatar)?
- Does it happen for **other** avatars too, or only the own?
- Firestorm at the same spot — does the reference show the same micro-drift
  (→ sim-side quantisation) or is it stable (→ our decode / ease)?

## Traced run on aditi (2026-09-15) — no drift

A logging-only trace of every source of the own avatar's facing (each
`ObjectUpdate`'s rotation and angular velocity, every body `SetRotation` sent,
the dead-reckoned and the rendered yaw, the camera aim) over an 80 s aditi
session: idle, then three ← / → turns. **The drift did not reproduce.** What
the trace does settle:

- **Idle, the simulator sends nothing.** From the login update at 1.8 s to the
  first turn at 57.8 s there is not one `ObjectUpdate` for the own avatar and
  not one `SetRotation`, and the rendered yaw never moved. So no source in this
  viewer turns an idle avatar on its own. The drift needs an input this run did
  not have: an update the sim chose to send, or a `SetRotation` from somewhere.
- **The simulator turns the body itself, and stops short.** A body rotation
  is not applied at once. SL streams the avatar turning towards it at a few
  rad/s (angular velocity 1–7 rad/s on the updates), then parks it with a final
  zero-angular-velocity update **1.5–3° from the heading asked for**:

  | heading sent | settled at |
  | --- | --- |
  | 157.27° | 154.38° |
  | 111.04° | 113.03° |
  | −58.25° | −56.79° |

  The rendered facing follows the settled value, so after every turn the body
  (and the camera that orbits it) sits a couple of degrees off the heading the
  viewer holds in `AvatarControls::yaw`. The next turn then starts from the
  held heading, not the drawn one.
- **Where the reference differs.** Firestorm does not draw its own avatar from
  the echoed rotation: `LLVOAvatar::updateOrientation` takes the forward
  direction from the agent's own at-axis for `isSelf()`
  (`agent.getAtAxis()`), and lets the pelvis lag the forward direction by up to
  `AvatarRotateThresholdSlow` (60°) before it turns at all. Neither a
  settle offset nor a small echoed step can show on its body.

A small echoed step, if the sim ever sent one to an idle avatar, would be
exactly the reported symptom under our path and invisible under the
reference's. The candidate fix is the reference's: draw the own avatar's facing
from the held heading rather than from the echo.

## The reference's facing, ported (2026-09-15)

The own avatar is now drawn, and followed by the third-person camera, at the
heading the viewer holds (`AvatarControls::held_heading`) instead of the
simulator's echo, once that heading is seeded from the first report. Every
other avatar, and the own one before its first report, still faces the echo.
Three things keep the held heading honest where the simulator, not the viewer,
turns the body:

- **Sitting** makes it follow the seated anchor's world facing (what the seat
  turns the body to), so the frame the avatar stands up it holds that facing
  and states it back once (the reference's `getOffObject` →
  `gAgent.resetAxes`). A first attempt re-seeded from the simulator's first
  standing report instead, and on aditi that stood the avatar up facing
  neither the seated direction nor the pre-sit one: the report is not the
  seated facing.
- **A server-steered update** (`FLAGS_SERVER_AUTOPILOT`, bit 24) is adopted as
  a forced heading (the reference's `gAgent.rotate` on the same flag).
- **A teleport arrival** already forced the heading (`crate::arrival`).

The movement driver now runs before the dead-reckoner, so a turn is drawn the
frame it is made. Tests: `physics::tests::
the_own_avatar_faces_its_held_heading_not_the_parked_echo`, and in the
world tier `standing_up_holds_the_facing_the_seat_left_the_body_at` and
`only_a_server_steered_report_turns_the_held_heading` (both fail on the old
driver).

Not ported: the reference's pelvis threshold (`AvatarRotateThresholdSlow`,
the body lagging a slow turn by up to 60°). That is an animation behaviour of
its own, not part of which heading is the truth.

## Verified live on aditi (2026-09-15)

A turn ends with the body square to the camera, an idle avatar does not turn,
and standing up leaves the avatar facing the way the seat had it, with the first
step walking that way.

**The original drift never reproduced**, so its cause is not proven. What is
proven is that an idle own avatar can no longer turn for anything the simulator
sends short of a server autopilot, a teleport arrival or a stand-up: its body
and camera no longer read the echo at all. If a periodic turn is seen again, it
comes from something that writes the held heading (`AvatarControls::yaw` — RLV
`@setrot`, the SpaceNavigator twist, mouselook aim) and that is where to look.
