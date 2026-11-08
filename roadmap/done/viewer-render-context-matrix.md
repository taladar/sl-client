---
id: viewer-render-context-matrix
title: Render context matrix — subjects × eye, time of day, mirror, layering
topic: viewer
status: done
origin: test-harness plan (2026-08-30); the coverage half of viewer-render-readback-tier
points: 8
refs: [viewer-render-readback-tier, viewer-water-transparency-scene-matrix, viewer-render-scene-coverage]
blocked_by: [viewer-render-pixel-oracle, viewer-plugin-groups, viewer-render-gpu-serialisation]
---

Context: [context/testing.md](../context/testing.md).

The water-translucency matrix proved the shape: a subject, a small set of
context axes, one verdict per cell. Generalise it so *any* registered
subject can be captured under *any* context without per-scene code.

- `RenderScene` gains `subject: Option<SubjectSignature { marker, points,
  bounds, translucent, emissive }>` and `applies: &[Axis]`; `None` keeps a
  self-staged scene on the bare rig. `SceneCx` gains `contexts:
  ContextSet { eye, time, viewpoint, layering, toggle, screen }` — one
  slot per axis, so a cell is at most one value per axis.
- `render_stage.rs::stage(scene, cx, …) -> Staged { camera, mirror,
  actor, plate }` wraps the subject: `Eye` places it relative to the sea
  and uses the matrix camera; `TimeOfDay` pins the environment to one of
  the four EEP anchors (or a custom day position); `Mirror` adds the probe
  sphere from `metallic-sphere-among-prims`; `InFrontOf`/`Behind` add a
  red actor of the requested kind; a matte white plate beside the subject
  is the lit reference for luminance orderings.
- `expectation(subject, cx) -> Visible | SeeThrough | Hidden | Report` is
  a rule table, not per-cell data; a scene override carries a reason.
- Sweeps in `render_matrix.rs`: R0 canonical (every subject once —
  coverage, health, declared symmetry, no logs), R1 single axis, R2 the
  curated pairs (glow over water, translucent in a mirror, avatar under
  water with fog, name tag behind glass, particles and the projector at
  midnight, HUD over world, translucent grazing over terrain, mirror under
  water, texture animation in a mirror). Each declares a capture budget the
  registry test enforces; `|applies| ≤ 4`.
- Teeth: `InFrontOf(OpaquePrim)` must read `Hidden`.

Once R1's `Eye` axis reproduces both `every_translucent_face_*` verdict
sets, the six `water-translucency-*` scenes collapse into one
`translucent-box-rack` subject.

The registry and the sweeps are in.
`SubjectSignature { marker, points, translucent, emissive }`, `applies` and
`ContextSet { eye, time, viewpoint, layering, toggle }` live in
`sl-viewer-render-fixtures` (`context.rs`); bounds are measured from the
geometry tier rather than declared. `render_stage.rs` stages, captures
(with and without the subject) and reads a cell; `render_matrix.rs` runs
R0, the four R1 sweeps (plus the time-of-day luminance ordering on a
tilted white plate), nine curated R2 cells and the opt-in all-pairs sweep,
under registry-enforced capture budgets. The six `water-translucency-*`
scenes are now the one `translucent-box-rack` subject.

Done (2026-11-08). The parts not built are now their own tasks:
[[viewer-render-hud-and-actor-axes]] (the `screen` slot and the
name-tag, particle, avatar and terrain actors), which the HUD-over-world,
name-tag-behind-glass and grazing-over-terrain pairs build on
([[viewer-render-matrix-hud-over-world]],
[[viewer-render-matrix-name-tag-behind-glass]],
[[viewer-render-matrix-translucent-grazing-terrain]]);
[[viewer-render-rig-glow-pass]], which glow over water needs
([[viewer-render-matrix-glow-over-water]]);
[[viewer-render-matrix-particles-projector-midnight]]; and
[[viewer-render-rig-sky-ambient]] for the shaded face that reads the same
at midday and midnight.
