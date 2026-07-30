---
id: viewer-perf-gpu-particle-sim
title: GPU compute particle simulation (+ raising the particle cap)
topic: viewer
status: deferred
origin: split out of viewer-perf-gpu-particles when Option 1 (GPU instancing)
  landed
refs: [viewer-perf-gpu-particles, viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

The GPU-instancing pass ([[viewer-perf-gpu-particles]], Option 1) moved the
render off the CPU: one shared unit-quad mesh, a compact per-particle
instance buffer, and camera-facing billboard expansion + PBR lighting in a
custom instanced pipeline. The **simulation** (emission + integration) stays
on the CPU. This task tracks the roadmap's Option 2 — moving the integration
itself onto the GPU — and is **deferred, not ready**: it should be picked up
only if a profile ever shows the integration (not the upload, which
instancing already removed) dominating.

Why it is parked rather than planned:

- **It is not currently a bottleneck.** The whole simulation is hard-capped
  at 4096 live particles (`LLViewerPartSim::sMaxParticleCount`), and a Verlet
  step per particle at that count is microseconds. Compute shaders pay off at
  10^5–10^6 particles; at 4096 the dispatch/sync overhead would cost more than
  it saves.
- **Only the kinematic substep is a clean fit.** The ballistic /
  acceleration / `BOUNCE` integration is embarrassingly parallel. But emission
  (`Emitter::emit`) is sequential per-source bookkeeping (burst timing, RNG
  stream, source-rotation accumulation, `max_age` death, the four emission
  patterns); the 4096 cap is a running total across all sources (a
  serialization point); and `TARGET_POS` / `TARGET_LINEAR` / `FOLLOW_SRC` need
  scene state (the source `GlobalTransform`, a resolved target object's world
  position) that lives in the ECS. A faithful GPU version is a hybrid: the
  kinematic patterns on the GPU, target/follow/emission still CPU-driven.
- **It costs testability.** The CPU sim is covered by deterministic unit tests
  (RNG, emitter, integrator) and the headless `render_test` harness reads
  particle positions on the CPU (the NaN-over-time check, the busiest-cloud
  debug camera). GPU-resident particles have no headless path and would need a
  readback stall to keep those.

## Raising the particle cap

The one change that flips this from "not worth it" to "the right tool" is
**deliberately exceeding the 4096 cap** — tens of thousands of particles for
dense weather / combat / club scenes. That is an intentional divergence from
the reference viewer's faithful limit, so it is a product decision, not just
an optimization: with the simulation on the GPU (append/consume buffers, a
free-list, atomic emission counters, indirect draw counts) the count could go
up by one or two orders of magnitude with the CPU cost staying flat. If that
is ever wanted, this task and the cap raise land together — the instance
render path from Option 1 already scales to it (the instance buffer is the
only per-frame upload and is linear in particle count).

## Also still open (from the Option 1 docstring)

The reference's **emission-rate LOD throttling** for distant / off-screen
sources (`LLViewerPartSourceScript::update`'s camera-distance area test) is
still not ported — the viewer runs every source at full rate regardless of
distance. Port it with the same phase-correct-resume care as
[[viewer-perf-texture-anim-pause]] so a source that comes back into range does
not visibly pop. Independent of the GPU-sim question above; grouped here as
the remaining particle-perf follow-up. This is also the real lever on the
**transparent-overdraw cost** of dense emitters (a big fountain is GPU-bound on
overlapping billboards, ~20 fps on the aditi test fountain — the same fragment
work the pre-instancing render did; instancing removed the CPU upload, not the
overdraw).

Particle ordering against the **translucent water surface** (fountains / wakes /
waterfalls / splashes) is its own bug — [[viewer-particle-water-ordering]].
