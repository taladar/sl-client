---
id: gridspec-lsl-physics-vehicles
title: LSL library behaviour on each grid — status flags, forces, targets and vehicles
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-physics-vehicles]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: Clamps and errors only (e.g. `llSetKeyframedMotion` constraints).
Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Status flags, forces, targets and vehicles.

## Fake grid

Large — [[server-lsl-lib-physics-vehicles]].

## Viewer

None.

## Done (2026-10-27)

- **Discover.** Seven probes, `sl-lsl-runtime/probes/physics/`: `status`
  (every `STATUS_*` flag of a fresh prim, masks of several flags, each flag
  set and cleared, values of 2 and −1, `PRIM_*` beside them,
  `llVolumeDetect` against the phantom flag, physics last); `material`
  (`llSetPhysicsMaterial` term by term in and out of range, the mass
  functions after each, the mass at other scales and physics shapes);
  `forces` (forces and torques read back, locally on a turned prim, the
  velocity and impulse calls on a prim that is not physical, then a short
  weightless physical leg); `targets` (position and rotation targets, their
  handles, a script's limit, ranges of 0 and −1, `llMoveToTarget` and the
  look-at functions physical and not, a target past 65 m, a tau of 0);
  `keyframed` (twenty-odd `llSetKeyframedMotion` tries, two of them in
  scripts of their own in prims B and C because OpenSim throws on them);
  `vehicles` (the setters' errors, a sled's motor, the motor clamps);
  `character` (pathfinding, Second Life only — OpenSim has none of it).
  Complete on aditi and both OpenSim engines.
- **Found on the way.** OpenSim keeps an impulse or a push given to a prim
  that is not physical and applies it when the prim turns physical, and
  throws on a keyframe list of the wrong types (YEngine stops the script).
  Both in `book/src/tools/lsl-probes.md`.
- **Document.** `book/src/gridspec/lsl.md` § Status flags, forces, targets
  and vehicles, linked from `content/scripts.md`.
- **Fake grid.** Large, as planned: the measured behaviour per flavour is in
  [[server-lsl-lib-physics-vehicles]].
- **Viewer.** None: no viewer surface reads these calls; a moving prim's
  object updates are rendered as any other's.
