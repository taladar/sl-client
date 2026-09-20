---
id: server-lsl-lib-physics-vehicles
title: Library tranche — status flags, forces, targets and vehicles
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-collision-and-physics]
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
