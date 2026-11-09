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

## From server-lsl-lib-ossl (2026-11-01)

OpenSim's own functions that shadow this tranche's are this tranche's to
implement, on the OpenSim flavours only (Second Life does not compile them): The
inertia functions, `osSetPrimFloatOnWater`, `osVolumeDetect` and
`osTeleportObject`. They are already in the library table with their signatures,
compiled by OpenSim's dialect, and **gated**: `vm::ossl::admit` checks the
region's `[OSSL]` settings before the function is looked for, so a denial is
already right and only the allowed call is missing. Port each from `OSSL_Api.cs`
(or `LS_Api.cs`), register it in `library/registry.rs` and move its line in
`sl-lsl-runtime/src/library/opensim-scope.txt` to `in`, which the coverage test
then holds; `ossl/available` and `ossl/errors` stop needing the answers their
replay writes in for it (`vm/tests/ossl.rs`). The 9: `osClearInertia`,
`osGetInertiaData`, `osSetInertia`, `osSetInertiaAsBox`,
`osSetInertiaAsCylinder`, `osSetInertiaAsSphere`, `osSetPrimFloatOnWater`,
`osTeleportObject`, `osVolumeDetect`.

## From server-lsl-lib-prim-state (2026-11-04)

`PRIM_PHYSICS`, `PRIM_PHANTOM` and `PRIM_TEMP_ON_REZ` are set and read through
the rule list (`library/prim_params`); `llSetStatus`/`llGetStatus` are not, and
the `prim/changed` replay (`vm/tests/prim_state.rs`) rewrites the probe's
`llSetStatus(STATUS_PHANTOM, …)` as `PRIM_PHANTOM` until they are. Drop that
rewrite when they land.

## From server-lsl-lib-task-inventory (2026-11-07)

A script's rez hands the host `Rez::die_at_edge` (OpenSim sets
`STATUS_DIE_AT_EDGE` on every object a script rezzes, Second Life not) and a
velocity; the fake grid keeps neither, having no status flags or physics. Keep
both here, and replay `inventory/rez`'s child with the real `llGetStatus` and
`llGetVel` rather than the values its test writes in.

## From test-lsl-differential-corpus-triage (2026-11-09)

Four parts of this tranche need no physics and landed with the corpus
triage, each held to its probe by a VM replay:

- **Status flags**: `llSetStatus` and `llGetStatus` over every `STATUS_*`
  flag, each flavour's multi-flag mask and `STATUS_CAST_SHADOWS` rule
  (`library/prim_state.rs`, `vm/tests/prim_state.rs`, `physics/status`). A
  physical prim only reads as one. OpenSim's script rez sets
  `STATUS_DIE_AT_EDGE` (the fake grid keeps it on the script's side of the
  prim); the `inventory/rez` replay reads the real flag.
- **`llVolumeDetect`**, as a flag only: the prim reads as phantom and a
  phantom clear leaves it so. The collisions it turns into detections are
  still this tranche's.
- **Targets**: `llTarget`, `llRotTarget`, their removals and the four events
  (`library/targets.rs`, `Engine::post_due_targets`, `vm/tests/targets.rs`,
  `physics/targets`' target phases): eight per script, Second Life's handles
  from the script's counter and OpenSim's from the region's local ids, the
  range rule, XEngine's `not_at_target` beside `at_target`, and OpenSim
  dropping targets at a state change (`events/state-drops`).
- **`llGetVel`**, as a read: the object's velocity as `OBJECT_VELOCITY` reads
  it, zero for every prim the fake grid has, since none is physical
  (`library/prim_state.rs`). `inventory/rez`'s child, rezzed at 5 m/s, reads
  zero on aditi and both OpenSim engines; its replay now calls the function.
  Physical motion is still this tranche's.

What remains: forces and impulses, `llMoveToTarget` and the look-at
functions, keyframed motion, vehicles, characters, material and mass, and
volume detect's collisions.
