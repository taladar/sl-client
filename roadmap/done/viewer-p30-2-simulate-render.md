---
id: viewer-p30-2
title: Simulate + render
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 30 — Particles
---

Context: [context/viewer.md](../context/viewer.md).

**P30.2. Simulate + render.** A CPU particle simulation mirroring
`LLViewerPartSim` / `LLViewerPartSourceScript` (emission patterns, wind,
acceleration, interpolation) rendered as camera-facing billboards
(`LLVOPartGroup`), textured via the texture pipeline. Net-new was an
extension of the `particles` module: an `Emitter` (port of
`LLViewerPartSourceScript::update` — the burst-timing accumulator, the
angular-velocity source rotation, the `max_age` death, and the DROP /
EXPLODE / ANGLE / ANGLE_CONE emission patterns, with a small deterministic
xorshift RNG standing in for `ll_frand`), a `Particle::integrate` (port of
`LLViewerPartGroup::updateParticles` — the velocity/accel Verlet step,
`TARGET_POS` / `TARGET_LINEAR` attraction, `BOUNCE`, `FOLLOW_SRC` drift, and
the colour / scale / glow interpolation), and `build_cloud_mesh` (port of
`LLVOPartGroup::getGeometry` — a camera-facing quad per particle with the
`FOLLOW_VELOCITY` re-orientation). The `drive_particles` system keeps one
`ParticleSim` cloud per source: a dedicated **world-space entity** (identity
transform, not a child of the source — mirroring `LLVOPartGroup` being its
own spatial object) whose dynamic mesh is rebuilt each frame from the live
particles, one `StandardMaterial` whose blend mode (additive vs alpha) and
unlit-ness come from the system's blend func + `EMISSIVE` flag, and its
texture pulled through the shared texture pipeline (or a procedural
soft-sprite default, the `sDefaultParticleImagep` counterpart, when the
source names none). The sim runs in Bevy world space; emission directions
are built in Second Life space and carried over by the single basis change,
with the source's SL-space rotation recovered from its Bevy
`GlobalTransform`. Deliberate simplifications (documented in-module): region
**wind** is not ingested (`WIND` is a no-op), the camera-distance rate
**throttle** is not ported (only the hard 4096 particle cap), `RIBBON` /
`BEAM` render as ordinary billboards, and a `TARGET_*` source falls back to
its own position (the reference's own fallback). Two cross-cutting facts
worth recording: (1) the cloud entity needs **`NoFrustumCulling`** — Bevy
computes a mesh's `Aabb` once when `Mesh3d` is added (from the then-empty
mesh), so a per-frame-rebuilt cloud is otherwise culled from every viewpoint
(the same reason `objects.rs` opts its rebuilt meshes out); (2) a debug
affordance `SL_VIEWER_PARTICLE_FOCUS=1` snaps the fly-camera to look at the
busiest particle cloud, so an unattended screenshot can frame a real emitter
without hand-aiming. **Live-verified on aditi:** a fountain's upward jets
render as continuous streams of camera-facing billboards (not brief flashes),
~2700 live particles across 28 sources spanning DROP / EXPLODE / ANGLE_CONE
patterns; clean build/clippy and 16 new unit tests over the RNG, emitter,
integrator, and mesh builder. As with P30.1, OpenSim's Default Region carries
no particle content, so the render is exercised on real SL.
