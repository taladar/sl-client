---
id: server-lsl-lib-detection-sensors
title: Library tranche — the detected block, sensors and raycasts
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-state-and-events, server-world-ecs-store,
  server-world-touch-and-grab, gridspec-lsl-detection-sensors]
refs: [server-world-collision-and-physics, server-world-agent-movement]
---

Context: [context/lsl.md](../context/lsl.md).

Four unrelated-looking event families — touch, collision, sensor and
`no_sensor` — share one mechanism: each fills a **detected list**, and
the `llDetected*` functions read entry *i* of whatever list the event
currently being handled brought with it.

- **The block**: `llDetectedKey`, `llDetectedName`, `llDetectedOwner`,
  `llDetectedType` (the `AGENT` / `ACTIVE` / `PASSIVE` / `SCRIPTED`
  bitfield), `llDetectedPos`, `llDetectedVel`, `llDetectedRot`,
  `llDetectedGrab`, `llDetectedGroup`, `llDetectedLinkNumber`,
  `llDetectedTouchFace`, `llDetectedTouchUV`, `llDetectedTouchST`,
  `llDetectedTouchPos`, `llDetectedTouchNormal`,
  `llDetectedTouchBinormal`. Reading past the end returns the type's
  default rather than erroring — one of the few places LSL is forgiving,
  and content depends on it.
- **Sensors**: `llSensor`, `llSensorRepeat`, `llSensorRemove`, and the
  `sensor(integer num_detected)` / `no_sensor()` pair. The sweep is a
  cone: an arc (radians) about the prim's forward axis, a range, a name
  and key filter, and a type mask. It returns **at most 16** results,
  **sorted by distance** — both limits are observable and both are easy
  to get wrong. A repeating sensor is a region-level timer, like
  `llSetTimerEvent`, and is cleared by a state change.
- **`llCastRay`**, with its `RC_*` options (reject types, data flags,
  max hits) and its documented per-region rate limiting. It needs the
  store's spatial index and the raycast from
  [[server-world-collision-and-physics]].
- **`llGetAgentList`** with its scope (region, parcel, parcel-owner)
  belongs with the avatar tranche but reads the same index.

The spatial query is the shared cost: a naive per-sensor scan over every
entity is fine for a scenario with fifty prims and is what the first
implementation should do — but it should go through the store's index
([[server-world-ecs-store]]) so the cost is one change later, not
sixteen call sites.

Acceptance: a sensor prim in the scripted scenario detects an avatar
walking into range and fires `no_sensor` when it leaves; a 16-result cap
and distance ordering are covered by a test with 20 targets; touch UV
and face on a clicked face match what the viewer picked; and
`llCastRay` against a fixture wall returns the expected hit point.

Already done by [[server-lsl-state-and-events]]: the detected block
(`Detected`, posted with `Engine::post_detected`) and all sixteen
`llDetected*` functions, with Second Life's out-of-range answers measured on
aditi. What is left here is who fills the block — the sensor sweeps, and the
touch and collision raisers' side of it.

## Measured by gridspec-lsl-detection-sensors (2026-10-25)

`book/src/gridspec/lsl.md` § Sensors, raycasts and touches is the table to
build to. The rows a flavour splits on:

- **Cap**: 32 results on Second Life, 16 on OpenSim (`SensorMaxResults`);
  nearest first by centre distance on both; the range held to 96 m.
- **Range edge**: a target exactly at the range is out on Second Life
  (`<`), in on OpenSim (`<=`). A range of 0 or below raises **no event** on
  Second Life, `no_sensor` on OpenSim.
- **Arc**: 0 and negative arcs mean "no arc" on Second Life (everything in
  range); on OpenSim 0 is "on the axis only" and a negative arc matches
  nothing.
- **Mask**: on Second Life `SCRIPTED` narrows (`ACTIVE | PASSIVE |
  SCRIPTED` returns no unscripted prim), and an idle scripted prim drifts
  between the `ACTIVE`/`SCRIPTED` and the `PASSIVE` answers while
  `llDetectedType` says 12 — the fake SL flavour may keep an idle scripted
  prim `PASSIVE` once its script has been idle a while, but must keep the
  narrowing. On OpenSim the mask is a union and a scripted prim is active.
  `AGENT_BY_USERNAME` matches nobody on OpenSim. An avatar's type is 3 on
  Second Life (35 wearing a scripted attachment), 1 on OpenSim.
  `llDetectedKey` past the end is `NULL_KEY` on Second Life only.
- **Repeat**: Second Life's first sweep comes one interval in, OpenSim's
  0.1 s later; an interval of 0 is one sweep on Second Life and none on
  OpenSim; the fastest is ~0.05 s on Second Life, 0.1 s on OpenSim; an
  `llSensor` during a repeat **ends the repeat** on Second Life and leaves
  it running on OpenSim; OpenSim delivers one queued sweep after
  `llSensorRemove`.
- **`llCastRay`**: Second Life returns positions as vectors and
  `[key, link, pos, normal]` with the data flags; it never hits the
  casting prim or a prim the ray starts inside, refuses `RC_MAX_HITS` 0,
  below 0 or over 256, an unknown option, `RC_REJECT_HOST` and a missing
  value with a `DEBUG_CHANNEL` line and an **empty list**, and answered 500
  calls in a row without an `RCERR_*`. OpenSim's default (V1) returns
  positions as **strings** and `[key, pos, link, normal]`, hits the
  casting prim and the prim it starts in, reports land past the ray's end
  (twice), treats `RC_MAX_HITS 0` as 1, ignores unknown options, and dies
  on a missing value. Whether the fake OpenSim flavour copies V1's land
  hits is a choice to make when this is built: they are a bug of OpenSim's,
  not a behaviour content relies on.
- **Touch**: both grids hand the viewer's `SurfaceInfo` to
  `llDetectedTouch*` unchecked, the `TOUCH_INVALID_*` values when there is
  none; Second Life raises one `touch` event per grab and release with no
  drag update between, OpenSim none.

## From gridspec-lsl-events (2026-10-25)

`events/merging` and `events/state-drops` (`book/src/gridspec/lsl.md`
§ Events): three `llSensor` calls in one handler answer **once** on Second
Life (each replaces the last) and three times on OpenSim; a repeating
sensor is dropped by a state change on every grid. On Second Life a sensor's
answer can come after a timer that fired 0.3 s after the call.
