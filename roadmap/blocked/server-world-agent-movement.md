---
id: server-world-agent-movement
title: The agent never moves — AgentUpdate is decoded and ignored
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
blocked_by: [server-world-heartbeat]
refs: [server-world-chat-routing, server-world-collision-and-physics,
  server-lsl-lib-avatar-control]
---

Context: [context/lsl.md](../context/lsl.md).

`SimSession` decodes a client `AgentUpdate` into
`ServerEvent::AgentUpdate(Box<AgentUpdateInfo>)` with the control flags
and camera state, and `sl-fake-grid` does nothing with it. The comment on
`driver.rs`'s `stands_on` says the quiet part: "The position is the one
the session was opened at, since the fake grid" — the avatar's position
is a constant for the whole session, and the only thing that moves it is
a scripted `Action` on a timeline.

On a real grid the client sends control flags and the *simulator* decides
where the avatar ends up; the client's own prediction is corrected by the
terse updates that come back. Everything positional a script can observe
therefore depends on this: `llDetectedPos`, `llSensor` over avatars,
`llGetAgentList`, parcel entry and exit, chat range
([[server-world-chat-routing]]), `llOverMyLand`, collisions, and
`llGetAgentInfo`'s `AGENT_WALKING` / `AGENT_FLYING` / `AGENT_IN_AIR`.

Wanted: an avatar controller run on the heartbeat that consumes the
control flags (`AGENT_CONTROL_AT_POS` and the rest of
`sl_wire::ControlFlags`) and produces a position, velocity and rotation:

- walk / run (`ALWAYS_RUN` and the `SetAlwaysRun` event, which the grid
  already receives), fly, up/down, turn-left/right, jump, and the
  `LBUTTON`/`ML_LBUTTON` bits touch routing wants;
- ground clamping against the region heightfield — `crate::terrain`
  already holds it, and `AVATAR_CENTRE_ABOVE_GROUND_M` already exists for
  exactly this offset;
- a terse update stream back to the client at the update rate, so the
  viewer's prediction is corrected rather than left to free-run, and so
  **other** sessions in the region see the avatar move at all (today an
  NPC is a fixture and a second real avatar never moves in the first
  one's view);
- `AGENT_CONTROL_STOP`, `AGENT_CONTROL_SIT_ON_GROUND` and stand, which
  the sit machinery ([[server-world-sit-and-attach]]) shares.

Explicitly **not** wanted here: a physics solver. Collision response,
falling, and being pushed are [[server-world-collision-and-physics]];
this task is the controller and the ground clamp, which is what a
kinematic character is on a simulator that has no vehicles yet.

The determinism rule bites here: the step must be `flags × step`, never
`flags × measured elapsed`, or two runs of one scenario put the avatar in
different places.

Acceptance: a client holding forward for N ticks arrives at a position
the test can compute from the step and the walk speed; the viewer, run
against the fake grid, walks; and chat-range and parcel-entry behaviour
follow the avatar rather than its login spot.
