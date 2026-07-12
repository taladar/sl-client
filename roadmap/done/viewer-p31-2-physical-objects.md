---
id: viewer-p31-2
title: Physical objects
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.2. Physical objects.** Give server-flagged physical prims (the
`LLViewerObject` physics flag / `LLPhysicsShapeType` — prim / convex hull /
none) an avian rigid body + collider derived from the prim / mesh geometry.
The sim stays authoritative — `ObjectUpdate` transforms drive the body while
avian smooths between updates and powers client-only dynamics. **Follow the
"Simulator authority & the Firestorm motion model" note above:** drive these
bodies **kinematically** (transform from `ObjectUpdate` + velocity/accel
dead-reckoning with the circuit-health phase-out and the ground /
region-height / off-region-edge clamps), **not** as free dynamic bodies under
the world gravity — otherwise a server object the sim has settled (and gone
silent about) keeps free-running in avian with no update to correct it.
Reserve avian's dynamic bodies for genuinely client-only motion
(Phases 32 / 34). **Done:** `objects.rs`'s `apply_object` now stamps every
server-flagged physical **root** prim (`FLAGS_USE_PHYSICS`, non-attachment —
attachments follow their wearer's joint, and linkset children ride the Bevy
hierarchy) with a `PhysicalObject` marker (the `apply_light` /
`apply_particles` insert-or-remove pattern), change-detected so a fresh insert
on every update reseeds. From that marker `physics.rs`'s
`drive_physical_objects` attaches a **kinematic** avian `RigidBody` + a
`Collider::cuboid` sized to the prim scale (rebuilt only on a genuine resize),
snaps the body to each authoritative update, and between updates dead-reckons
the pose forward as a faithful port of
`LLViewerObject::interpolateLinearMotion`: the
`(vel + 0.5*(dt - PHYSICS_TIMESTEP)*accel) * dt` extrapolation (scaled by
region time dilation, the reference's `idleUpdate`
`dt = time_dilation * dt_raw`), the `applyAngularVelocity` spin, the
circuit-health **phase-out** (ramps `1 → 0` between 2 s and 3 s of silence
*only once the circuit looks stalled* — a new `CircuitLiveness` resource
tracking the last inbound event stands in for `LLCircuitData::isBlocked` / the
last-packet time, so "quiet because prediction holds" keeps going while "quiet
because the sim lags" eases to a halt), and the geometric clamps
(region-height ceiling, a permissive `getMinAllowedZ` ground floor from a new
`TerrainState::land_height` land lookup, and the off-region-edge clip /
region-crossing cap that zero velocity when a prediction would leave into a
void vs. a known neighbour — neighbours read from the time-dilation-seen
region set, the `clipToVisibleRegions` analogue). Kept viewer-only (no
runtime-parity obligation, like the P31.1 world). The whole extrapolation is
per-component `f32` / `Quat`-method math to satisfy the workspace
`arithmetic_side_effects` lint. Verified: clean build/clippy + 12 unit tests
(dead-reckon formula, phase-out ramp/gating, angular step, the ceiling /
floor / void-clip / region-crossing clamps, ground-floor radius, neighbour
lookup) and a **live OpenSim** run — a 1 m physical box dropped mid-session (a
`<Flags>Physics</Flags>` OAR merge-loaded while the viewer was already
streaming, so it fell live under the region's `ubODE` engine) was received
flagged physical, given a `1.00×1.00×1.00 m` kinematic body, and dead-reckoned
through its fall onto the avatar (user-confirmed on screen), with a clean quit
and no panics / avian / schedule errors. Two aspects are deliberately deferred
to their own points below: the CAPS `LLPhysicsShapeType` (prim / hull / none)
and a real geometry-derived collider (the P31.2 collider is a scale-sized
cuboid regardless) → **P31.3**; and dead-reckoning of **avatars** (a separate
`avatars.rs` path) → **P31.4**.
