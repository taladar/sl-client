---
id: viewer-test-baseline-format
title: One baseline format for UI and render facts
topic: viewer
status: ready
origin: test-harness plan (2026-08-30) — the shared format both baseline tiers wait on
points: 3
refs: [viewer-ui-baseline-regressions, viewer-render-baselines]
---

Context: [context/testing.md](../context/testing.md).

[[viewer-ui-baseline-regressions]] and [[viewer-render-baselines]] both
require a committed recording of *derived intent* that may not drift by
accident, and both say the other must share its format. Build the format
once, in `sl-viewer-testkit/src/baseline.rs`:

- `Fact::{Int, Float { value, tolerance }, Text, Vec2, Vec3}`;
- `Baseline { schema, blessed_describe, blessed_at, facts: BTreeMap }`
  (sorted, so diffs are stable; the describe is provenance, never
  compared);
- `compare(recorded, current) -> Vec<Drift>` and `check(path, current)`
  — a missing file fails with the bless command in the message, and
  `SL_VIEWER_BLESS_BASELINES=1` rewrites it (the settings-golden flow).

Layout `baselines/<crate>/<tier>/<id>.toml`, one file per subject, one
canonical cell. A per-crate test fails on an orphan file. First facts:
prim vertex/triangle counts per LOD for the tessellated scenes, the
avatar's declared bounds, each scene's subject-centre framing pixel
(CPU projection), pie option angles, floater default sizes.
