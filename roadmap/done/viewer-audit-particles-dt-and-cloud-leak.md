---
id: viewer-audit-particles-dt-and-cloud-leak
title: Particle integration uses an unclamped dt, and a switched-off emitter leaks its cloud
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

Two defects in `sl-viewer-world-scene/src/particles.rs`:

- `:955` — the integration step uses an **unclamped** `dt` while the emitter
  side is capped at `:480`. The reference caps at `0.1f`
  (`llviewerpartsim.cpp:721`). After a decode or region-crossing hitch every
  particle jumps in one step, and a `BOUNCE` particle tunnels through its plane.
- `:1164` — an emitter switched off (`llParticleSystem([])`) leaks its `Cloud`
  forever. The only `remove::<Cloud>` is the HUD-disabled branch (`:977`), and
  `drive_particles` requires `&ObjectParticleSystem` (`:934`), so the entity is
  never revisited. Its `Vec<Particle>`, cloned `ParticleSystem` and per-cloud
  `Handle<Image>` live until the prim despawns — while the doc two lines up
  asserts the opposite.

Adjacent per-frame cost worth fixing in the same pass: `:1065` allocates a fresh
`Vec` per cloud per frame via `build_cloud_instances`, then re-`insert`s
`ParticleInstances` and `ParticleDrawParams` (cloning the texture handle). A
`&mut ParticleInstances` written in place (`clear()` + `extend`) reuses the
allocation and still marks `Changed`.

## Resolution (2026-09-14)

**The step is capped once, for the whole simulation.** The reference caps in
exactly one place — `LLViewerPartSim::updateSimulation`'s
`llmin(update_timer.getElapsedTimeAndResetF32(), 0.1f)` — and hands that one
`dt` to both the emitter and the integration
(`LLViewerPartSourceScript::update(dt)`), so `drive_particles` now does the
same through a `sim_dt` helper over a `MAX_SIM_DT` constant. The emitter's own
burst-backlog clamp stays: it bounds a *different* thing (how many bursts one
step may catch up on).

The artifact the cap prevents is not a tunnel but an equally wrong mirror:
`BOUNCE` reflects the particle's position about the source plane *after* the
step, so however far the step carried it through is however far above the plane
it is thrown. A one-second step at 5 m/s puts a particle 4 m below the plane
and mirrors it to 4 m above — a 3 m teleport in a single frame.

**A stopped source drops its cloud.** `retire_orphaned_clouds` gained a second
query — `With<Cloud>, Without<ObjectParticleSystem>` — and removes the `Cloud`
those sources still carry. It is the right system for it: it already exists to
reap what the ECS cannot, it already runs after the driver, and both halves ask
the same question. The three doc comments that asserted this already happened
now describe what does.

**The per-frame render inputs are written in place.** `build_cloud_instances`
takes `&mut Vec<ParticleInstance>` and does `clear()` + `extend`, and
`drive_particles` looks the cloud's render entity up in a new disjoint query to
write both it and `ParticleDrawParams` through `&mut`. A cloud entity spawned
this same frame is not in that query yet, so it still gets both through
`Commands` — unchanged from the reader's point of view, since the render world
extracts both components every frame regardless of change detection.

Unit-verified, four tests in `particles.rs`: `the_simulation_step_is_capped`
(including a NaN delta, which `f32::min` lands on the cap),
`a_bounce_particle_survives_a_frame_hitch` (one unclamped step moves the
particle further than a whole capped step's travel; ten capped steps never do),
`instances_are_built_in_place` (a shrunken cloud leaves no stale record and
keeps its allocation), and the two cloud-lifetime tests —
`orphaned_cloud_renders_are_retired` now also asserts the still-in-world source
drops its `Cloud`, and `a_restarted_source_is_seeded_again` pins that a live
source keeps the one it was seeded.
