---
id: server-world-agent-movement
title: The agent never moves — AgentUpdate is decoded and ignored
topic: server
status: done
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

## From server-world-sit-and-attach (2026-11-02)

Sitting is world state now (`sl-fake-grid/src/seating.rs`), and `sit-stand`
runs offline but for its walks and flights: put its far legs (`far_legs`,
and `target_legs`' 40 m sit) on the fake flavours once an avatar moves, with
Second Life's reach edge (`SECOND_LIFE_PLAIN_REACH_M`, 8.4 m between the 7.5 m
answered and 9.3 m not) held by them, and the drop to the ground after a
stand.

## As built (2026-11-06)

`sl_fake_grid::movement`, run by the heartbeat as its third step for every
root agent in the region, in session order: each tick folds the latest
`AgentUpdate` controls and body rotation (and `SetAlwaysRun`) into a
`Motion`, advances it by exactly the step, writes the result into the
session's placement (so chat range, the parcel an edit is pushed to and
everything else that reads it follow the avatar), moves what the agent wears
with it, and reports it in an `ImprovedTerseObjectUpdate` when one is due.

- **The controller**: walk, run, crouch and crouched walk, fly, climb,
  descend, jump, fall, land; `STOP`, `SIT_ON_GROUND` and `STAND_UP`; ground
  clamping on the heightfield at `AVATAR_CENTRE_ABOVE_GROUND_M`; the region's
  edge stops a walk. A seated agent is left alone, and one stood up above the
  ground drops to it.
- **Per flavour** (`ImitatedGrid::movement_policy`, a row of the module
  table): every measured speed; a gravity derived from each grid's jump
  (`v² / 2h`), which also lands a 420 m fall in the measured time once the
  measured fastest fall is reached; coming to rest over 0.6 s or 0.3 s;
  Second Life's holds (a pre-jump and a hard landing wait for the viewer's
  `FINISH_ANIM`) against OpenSim's timed ones; and the update cadence —
  every tick, or every two seconds, for a walk; nothing, or three a second,
  for a steady flight; one and a half a second, or nothing, at rest — with
  any change of motion reported in the tick it happens. Zero acceleration,
  the ground's plane under a standing avatar and `0 0 0 1` in the air.
- **The animation set follows the motion**: the set's default is the
  locomotion built-in (`AnimationSet::set_default`), stated to the agent when
  it changes, one at a time as OpenSim states it.
- `agent-movement` runs offline against both flavours, and `sit-stand` runs
  every leg there, its far walks and its 40 m flight included. `e2e_movement`
  runs on the fake grid (the Second Life flavour, so the viewer's
  `FINISH_ANIM` for its pre-jump is exercised) as well as on a live one.

Acceptance, held by tests: `holding_forward_walks_the_step_times_the_speed`
(the position after N ticks is `N × step × walk speed`, on both flavours);
the viewer walks (`e2e_movement`, `e2e_environment`'s parcel layer); chat
range and parcel entry follow the avatar
(`walking_over_a_parcel_line_pushes_each_parcel`, and the parcel push of
[[server-fake-grid-parcel-on-movement]]).

### Not done — and why

- **Other sessions do not see the avatar move**: residents are not shown to
  each other at all yet ([[server-fake-grid-agent-avatars-shared]]); the
  terse updates go to the agent's own session.
- **No walking over a region border**: a border is a crossing, which this
  controller does not start; it stops the avatar at the edge.
- **No prims to stand on or bump into**: the ground is the heightfield alone
  ([[server-world-collision-and-physics]]).
- **Second Life's arrival hold** (one login of three held in `land` until a
  `FINISH_ANIM`) is not imitated: what makes an arrival held was not
  measured.
- **Second Life's extra animation assets** beside the built-ins, and its
  inexact report of a turn, are not imitated; OpenSim's one built-in at a
  time is, on both flavours.
- **Swimming and mouselook steering** were not measured.
