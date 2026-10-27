---
id: server-lsl-lib-physics-vehicles
title: Library tranche — status flags, forces, targets and vehicles
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-collision-and-physics,
  gridspec-lsl-physics-vehicles]
refs: [server-physics-integration, server-lsl-lib-prim-state]
---

Context: [context/lsl.md](../context/lsl.md).

The library half of [[server-world-collision-and-physics]], and the place
where that task's "kinematic solver, vehicles deferred" decision gets
revisited with a concrete bill.

- **Status flags**: `llSetStatus` / `llGetStatus` over `STATUS_PHYSICS`,
  `STATUS_PHANTOM`, `STATUS_ROTATE_X/Y/Z`, `STATUS_BLOCK_GRAB`,
  `STATUS_DIE_AT_EDGE`, `STATUS_RETURN_AT_EDGE`,
  `STATUS_CAST_SHADOWS`, `STATUS_BLOCK_GRAB_OBJECT`. Most map onto
  `PrimFlags` the `Object` record already carries;
  `STATUS_DIE_AT_EDGE` is a region rule, not a prim one.
- **Forces and impulses**: `llSetForce`, `llGetForce`,
  `llApplyImpulse`, `llApplyRotationalImpulse`, `llSetTorque`,
  `llSetForceAndTorque`, `llPushObject`, `llSetBuoyancy`,
  `llSetHoverHeight`, `llStopHover`, `llGroundRepel`,
  `llSetVelocity`, `llSetAngularVelocity`.
- **Targets**: `llTarget` / `llTargetRemove` with `at_target` /
  `not_at_target`, `llRotTarget` / `llRotTargetRemove` with the rot
  pair, `llMoveToTarget` / `llStopMoveToTarget`, `llLookAt` /
  `llStopLookAt`, `llRotLookAt`. These are the functions scripted
  movement actually uses, far more than raw impulses, and they are
  cheap on a kinematic solver.
- **Volume detect**: `llVolumeDetect`, which turns collisions into
  pass-through detections — the basis of most scripted sensors, doors
  and traps.
- **Vehicles**: `llSetVehicleType`, `llSetVehicleFloatParam`,
  `llSetVehicleVectorParam`, `llSetVehicleRotationParam`,
  `llSetVehicleFlags`, `llRemoveVehicleFlags`. This is a whole
  simulation model of its own (linear and angular motors, friction
  timescales, banking, hover, reference frame) and a kinematic solver
  cannot fake it convincingly. The honest options are: implement the
  parameter model over a real rigid-body engine, or **stub the setters
  and document that vehicles do not move** so a script that configures a
  vehicle at least compiles and runs. Decide when this tranche is
  picked up, with the scripted-scenario needs in hand; do not decide it
  now.

Two more movement surfaces belong here and are easy to forget:
**`llSetKeyframedMotion`** (a list of position/rotation/time triples the
simulator plays back — the modern way scripted doors, lifts and rides
move, and a very good fit for a kinematic solver, so it may be the
cheapest high-value item in this tranche) and the **pathfinding /
character** API (`llCreateCharacter`, `llDeleteCharacter`,
`llNavigateTo`, `llPursue`, `llEvade`, `llWanderWithin`,
`llPatrolPoints`, `llExecCharacterCmd`, `llUpdateCharacter`, and the
`path_update` event), which needs a navmesh the fake grid has no reason
to build. Treat keyframed motion as in scope and the character API as a
stub-and-document case, alongside vehicles.

Acceptance: a prim with `STATUS_PHYSICS` falls to the ground and rests;
`llMoveToTarget` reaches its target within tolerance and raises
`at_target`; `llVolumeDetect` produces collision events while letting an
avatar through; and the vehicle decision is recorded with its reason.

## Measured (gridspec-lsl-physics-vehicles, 2026-10-27)

`book/src/gridspec/lsl.md` § Status flags, forces, targets and vehicles has
the tables; what the flavours must differ in:

- **Status.** `llGetStatus` of several flags is TRUE if any is set on Second
  Life, 0 on OpenSim. `STATUS_CAST_SHADOWS` cannot be set on Second Life; on
  OpenSim either value sets it for good.
- **Material and mass.** An out-of-range term refuses the whole
  `llSetPhysicsMaterial` call on both, with `ERROR: Failed to set material
  properties.` on Second Life's debug channel and silently on OpenSim.
  Second Life's mass follows density (and, late, scale); OpenSim's stays at
  the rezzed prim's.
- **Forces.** A local force or torque reads back in the prim's frame on
  Second Life and in the region's on OpenSim. OpenSim stores impulses and
  pushes given while not physical and applies them on turning physical.
- **Targets.** Eight per script on both; a range of 0 is never reached on
  Second Life and always at the target on OpenSim (`<=` on squares); XEngine
  runs `not_at_target` beside `at_target`. `llMoveToTarget` past 65 m does
  not move on Second Life and does on OpenSim. `llLookAt` and `llRotLookAt`
  turn a prim that is not physical on both.
- **Keyframed motion.** Second Life refuses, with a debug-channel line each,
  the default physics shape, frame times under 0.1 s, malformed lists,
  unknown modes and options, `llSetPos` and physics during a motion; OpenSim
  plays or silently ignores all of these, and throws on wrong element types.
  A stop leaves the prim where it is on both.
- **Vehicles.** Second Life answers unknown parameter codes on the debug
  channel and nothing else; neither grid clamps the linear motor at 30 m/s.
- **Characters.** Second Life only (errors and `path_update` codes in the
  table); OpenSim does not compile a script that names them.
