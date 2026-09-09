---
id: viewer-arrival-orientation-snap
title: Avatar arrives facing the wrong way then snaps to the correct orientation (rotates the whole minimap)
topic: viewer
status: done
origin: user report while live-testing the event-queue redesign on local OpenSim (2026-08-07)
---

Context: [context/viewer.md](../context/viewer.md).

On arriving in a region (crossing or teleport) the avatar appears in some
default / stale orientation and then, a moment later, **turns to its correct
facing**. Because the minimap is oriented to the avatar's heading, the whole
minimap rotates on every arrival — a jarring visual.

## ROOT CAUSE (2026-09-09): the arrival facing was thrown away

The simulator states the pose it placed the agent at in the
`AgentMovementComplete` that confirms the arrival (`Data.Position` /
`Data.LookAt`) — and on a teleport it has *already turned the avatar* to it
(OpenSim `ScenePresence.CompleteMovement` → `RotateToLookAt(look)`). The client
parsed only `Data.RegionHandle` from that message and dropped the rest, and the
intra-region `TeleportLocal` likewise surfaced only its position.

Nothing else in the viewer knows the avatar's facing: it is read from the
`ObjectUpdate` stream (`AvatarMotion`), the third-person camera orbits the
facing it finds there, and the minimap is oriented to that camera. Avatars are
not purged by a `world_reset`, so after a teleport the own avatar entity stands
at its **pre-teleport** heading until the destination's first object update for
it lands — then the body turns, the rear-view camera swings around it (the eye
is computed *from* the facing) and the minimap rotates with the camera. The
P31.7 rotation ease spreads that turn over ~80 ms, which is why it reads as a
turn rather than a jump.

## FIX LANDED (2026-09-09)

The reference does not wait for the echo either:
`process_agent_movement_complete` slams the agent frame to the stated look-at
(`gAgentCamera.slamLookAt` → `LLAgent::resetAxes`) and re-seats the camera on
the avatar **without animating** (`setFocusOnAvatar(true, false)`);
`process_teleport_local` does the same for an intra-region teleport.

- `sl-proto` now carries the arrival pose: a new `Event::AgentArrived`
  (`region_handle` / `position` / `look_at` / `teleport`) from every
  `AgentMovementComplete`, and `Event::TeleportLocal` gained its `look_at`.
  `teleport` is `true` only for the child-circuit confirmation a teleport
  commits on — the root-circuit path is the initial login or a **crossing**.
- `sl-viewer-world-view/src/arrival.rs` (`slam_arrival_facing`) applies it:
  the own avatar's authoritative `AvatarMotion.rotation`, its *rendered*
  orientation (`AvatarInterp::rendered_rotation`, so the P31.7 ease has nothing
  to glide through), the walk heading (`AvatarControls::forced_heading`, which
  the movement driver advertises at once — the reference's
  `send_agent_update(true, true)`), and the camera rig (un-seeded, so the follow
  snaps behind the new facing instead of orbiting around to it; `aim_along` for
  mouselook, where the camera *is* the facing). Ordered between the avatar
  object fold and the dead-reckoner, so a stale update in the same batch loses.

Only a **teleport** arrival slams, as in the reference: a crossing carries the
facing over the border (OpenSim's `m_gotCrossUpdate` suppresses its own
`RotateToLookAt` there), so re-applying a restated look-at would itself be a
snap. A degenerate look-at (no horizontal component) is ignored — OpenSim
substitutes the velocity and then a fixed default when it has no facing to
report.

Tests: `arrival.rs` unit + ECS tests (a teleport arrival turns the avatar,
its rendered orientation and the camera rig included, that frame; a crossing
leaves it alone; degenerate and non-teleport look-ats state no heading),
`world_test.rs::a_teleport_arrival_turns_the_body_before_any_object_update`
(the same through the viewer's real plugin wiring, down to the body rotation
re-stated to the simulator), `sim_session.rs::inter_region_teleport_two_sims`
asserts the destination's stated placement reaches the client, and the
fake-grid `client_end_to_end` local-teleport test pins the look-at onto
`TeleportLocal`.

**Needs live verify** (local OpenSim, two regions): teleport between regions and
confirm the avatar is facing the right way on the first frame it is visible and
that the minimap does not rotate on arrival; then walk a border crossing and
confirm nothing turns there either.

Not covered, deliberately: the reference also slams the arrival **position**
(`setPositionAgent` / `slamPosition`). Ours snaps the rendered translation on
any region change or region-scale jump already (`TRANSLATION_SNAP_DISTANCE_M`),
and the report was explicit that only the facing was wrong.
