---
id: viewer-object-target-omega-spin
title: Client-side llTargetOmega spin for non-physical prims
topic: viewer
status: ready
origin: script-interface survey (2026-07-23)
refs: [viewer-p31-2]
---

Context: [context/viewer.md](../context/viewer.md).

`llTargetOmega` on a non-physical prim is a purely client-side effect:
the sim sends the angular velocity in the object update and each viewer
integrates the rotation locally (nothing moves server-side). Our
`physics.rs` applies `angular_velocity` only to objects carrying
`FLAGS_USE_PHYSICS` (verified: `is_physical_root` gates the
`PhysicalObject` marker), so spinning signs, windmills, fans, and dance
balls — the overwhelmingly common non-physical case — never rotate.

Scope:

- Drive a continuous local rotation for non-physical objects with a
  non-zero `angular_velocity`, integrating per frame like the reference
  (`applyAngularVelocity` in the object updater).
- Child-prim spin composes with the parent transform (spinning child in
  a static linkset); attachments spin too.
- Zeroed angular velocity (script calls `llTargetOmega` with gain 0 or
  spin 0) stops the spin and leaves the current authored rotation.
- Keep the physical-object path untouched — spin there already comes
  from the dead-reckoning integrator.

Reference (Firestorm, read-only): `LLViewerObject::processUpdateMessage`
angular-velocity handling and `llviewerobject.cpp` client-side rotation
integration.

Builds on: the object update pipeline and the existing `angular_step`
integrator in `physics.rs` ([[viewer-p31-2]]).
