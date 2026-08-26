---
id: viewer-audit-particles-dt-and-cloud-leak
title: Particle integration uses an unclamped dt, and a switched-off emitter leaks its cloud
topic: viewer
status: bugs
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
