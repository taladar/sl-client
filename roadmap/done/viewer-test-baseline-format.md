---
id: viewer-test-baseline-format
title: One baseline format for UI and render facts
topic: viewer
status: done
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

Done (2026-08-31). `sl-viewer-testkit/src/baseline.rs` holds the format,
the comparison, the bless flow, the `baselines/<crate>/<tier>/<id>.toml`
layout, a `Facts` builder and the orphan sweep, self-tested against a
committed fixture under its own `self-test` tier. The first facts landed
with it: ten opt-in render scenes (per-LOD vertex and triangle counts,
world extents, and the CPU-projected framing pixel of the subject's
centre) and the pie menu's measured compass angles. The CPU projection
is held to the readback rig's own camera by a GPU-tier test, so a
recorded framing pixel cannot drift away from the picture.

Two of the listed first facts did **not** land, deliberately:

- **Floater default sizes.** Every floater is spawned by its own plugin
  from an inline `FloaterSpec`, and the composition root adds those
  plugins one by one — there is no `ViewerUiPlugins` group yet (the
  context doc names one; the code has four groups, none of them the UI's),
  so nothing can stand the floaters up headlessly to measure them.
  Blocked behind that extraction ([[viewer-ui-shell-plugin-groups]])
  rather than worked around with a hand-maintained list of specs, which is
  the duplication a baseline is supposed to remove.
- **Declared bounds as such.** A scene's `DeclaredBounds` is a
  declaration in the source; re-recording it would pin the code against
  itself. The avatar scenes record their **measured** extents instead,
  which is the fact that moves when a morph, a skeleton or a basis
  changes.
