---
id: viewer-render-context-matrix
title: Render context matrix — subjects × eye, time of day, mirror, layering
topic: viewer
status: in-progress
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

In progress (2026-08-30): the first slice is in
`sl-client-bevy-viewer/src/render_matrix.rs` — the R0 sweep (every
subject scene must paint its measured silhouette, exclusions listed with
reasons and guarded), `capture_with`'s staging hook, the
opaque-wall-hides-the-subject teeth pair and the first underwater-eye
cell. The registry fields (`SubjectSignature`, `applies`), the stage
builders and the R1 sweeps are still to grow.
