---
id: server-lsl-lib-detection-sensors
title: Library tranche — the detected block, sensors and raycasts
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-state-and-events, server-world-ecs-store,
  server-world-touch-and-grab]
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
