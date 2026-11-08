---
id: viewer-render-matrix-particles-projector-midnight
title: Render matrix — particles and the projector as subjects at midnight
topic: viewer
status: ready
origin: viewer-render-context-matrix (2026-11-08) — curated pairs not built
points: 3
refs: [viewer-render-context-matrix, viewer-projector-lights-spot-shadows]
---

Context: [context/testing.md](../context/testing.md).

Two of the context matrix's planned curated pairs: the particle fountain,
and a projector's footprint, staged at `TimeOfDay::Midnight`. Both are what
should still read in the dark: emissive particles, and light a projector
throws on a surface no sun reaches.

Both scenes are **self-staged** today (`particles-fountain` and
`projector` have `subject: None`). The matrix measures a subject's bounds
from the geometry tier's tessellation, and neither has geometry to
tessellate: a particle system's extent comes from its simulation, and a
projector's subject is the patch of a wall its frustum lights. So the work
is a subject form for them first:

- give `SubjectSignature` (or `measured_bounds`) a source of bounds for a
  geometry-less subject — the simulated particle extent at the capture time
  and the projector frustum's footprint on its receiver — without falling
  back to hand-declared boxes for meshed subjects;
- register both with `applies: [Time]` and add the midnight cells to
  `CURATED`;
- expectation: visible by difference at midnight, and for the projector
  the footprint brighter than the same receiver with the projector hidden.
  Their own luminance ordering is inverted against a lit subject's, since a
  light source does not dim with the sun, so state that as a rule rather
  than exempting them.
