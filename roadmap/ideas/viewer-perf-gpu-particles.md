---
id: viewer-perf-gpu-particles
title: GPU-instanced particle rendering
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling, viewer-perf-frame-churn-cleanups]
---

Context: [context/viewer.md](../context/viewer.md).

The particle pipeline (`particles.rs:973-1185`) is fully CPU-side: every
frame, per particle source, it integrates all live particles and then
**rebuilds the entire billboard mesh** — five attribute `Vec`s via
`build_cloud_mesh` (`particles.rs:867`) — and `meshes.insert`s it
(`particles.rs:1174`), a full vertex-buffer re-upload per source per
frame.

Simulation on the CPU is defensible (LL particle semantics are complex
and counts are bounded), but the mesh rebuild + upload is the classic
case for **GPU instancing**: upload one quad, feed per-particle instance
data (position, scale, rotation, color, UV frame), and expand billboards
in the vertex shader facing the camera. The per-frame CPU work then
shrinks to writing a compact instance buffer (or even just simulation
outputs), and the five-attribute mesh churn disappears.

Options, in ascending ambition:

1. **Instance buffer, CPU sim** — keep the existing simulation, replace
   `build_cloud_mesh` + `meshes.insert` with a per-source instance
   buffer (custom material/pipeline; Bevy 0.19's system-based renderer —
   check the migration guides before designing, per the skill note).
   Most of the win, least risk.
2. **GPU simulation (compute)** for the simple kinematic patterns
   (ballistic + wind + drag), falling back to CPU for pattern types that
   need scene queries (target-omega, follow-source). Only worth it if
   profiling shows integration itself (not the upload) dominating.

A related quick fix — the per-frame `ops.system.clone()` of the whole
`ParticleSystem` struct per source (`particles.rs:1029`) — is filed in
[[viewer-perf-frame-churn-cleanups]] and should land first; it is
independent of the render path.

Also worth porting while in here: the reference's emission-rate LOD
throttling for distant/off-screen sources (deliberately not ported so
far, per the module docstring) — with the same phase-correct-resume
care as [[viewer-perf-texture-anim-pause]] so returning sources don't
visibly pop.

## Estimated impact

Medium-high on particle-dense scenes (clubs, combat sims, weather):
removes per-source per-frame mesh rebuild + full vertex upload; CPU cost
becomes proportional to particle count only (integration), GPU upload
drops from 4 verts × 5 attributes per particle to one compact instance
record. Larger effort than the other perf tasks (custom render plumbing)
— schedule after the cheap wins. Baseline first via [[viewer-profiling]]
(particle system zone + upload sizes on a heavy-emitter test scene).

Confidence: high on the current cost structure (verified); medium on
implementation effort (Bevy 0.19 custom instancing has churned across
versions).
