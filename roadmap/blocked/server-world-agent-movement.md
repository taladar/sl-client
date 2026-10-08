---
id: server-world-agent-movement
title: The agent never moves — AgentUpdate is decoded and ignored
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
blocked_by: [server-world-heartbeat, gridspec-neighbours-crossing,
  gridspec-agent-movement]
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

Measured by [[gridspec-agent-movement]] (2026-10-08), in
`book/src/gridspec/movement.md` — build to these, per flavour, as rows of
`ImitatedGrid`:

- the speeds of each control (a walk is 3.20 m/s on Second Life and 3.145
  on OpenSim, level flight 16.0 against 12.58, a descent 22.8 against 16.35;
  a crouch slows only Second Life's avatar; `FAST_AT` does nothing on
  either);
- how far the avatar carries on when a control is let go;
- **Second Life's holds**: the avatar does not leave the ground until its
  `pre_jump` is answered with `FINISH_ANIM`, and does not get up from a hard
  landing (or from an arrival at a landing point) until that is; OpenSim
  runs both on its own clock and ignores the bit;
- the update cadence: Second Life reports a walking avatar eight to sixteen
  times a second and a steadily climbing one not at all, OpenSim a level
  walk once every two seconds; the velocity is the motion and the
  acceleration is always zero;
- the collision plane: the ground's on the ground, `0 0 0 1` in the air.

The `agent-movement` conformance case is live-only until this lands: add it
to `OFFLINE_CASES` for both flavours here, and run `e2e_movement` (which
needs `Need::LiveGrid` today) against both.

Acceptance: a client holding forward for N ticks arrives at a position
the test can compute from the step and the walk speed; the viewer, run
against the fake grid, walks; and chat-range and parcel-entry behaviour
follow the avatar rather than its login spot.
