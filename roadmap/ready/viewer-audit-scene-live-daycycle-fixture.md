---
id: viewer-audit-scene-live-daycycle-fixture
title: A test fixture that advances the day cycle between samples
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 5
refs: [viewer-audit-scene-change-guards-day-cycle, viewer-audit-probe-ambient-multiply, viewer-audit-tonemap-legacy-sky]
---

Context: [context/viewer.md](../context/viewer.md).

Three separate scene defects — [[viewer-audit-scene-change-guards-day-cycle]],
[[viewer-audit-probe-ambient-multiply]] and [[viewer-audit-tonemap-legacy-sky]]
— are all **correct under the screenshot harness and wrong on a live grid**,
because the harness pins `SL_VIEWER_SKY_DAY_POSITION` and a real grid does not.

That is a coherent blind spot rather than three unrelated slips, and it argues
for a fixture that advances `day_position` between samples so a guard which
never holds under a moving sky is a test failure rather than a profiler finding.

Scope: a scene test app that steps the day cycle across frames and asserts
(a) `TerrainMaterial` is **not** marked modified when the lighting has not
meaningfully changed, (b) `GlobalAmbientLight` converges rather than decaying
under a non-zero `probe_ambient_scale`, and (c) a legacy sky produces a zero
tonemap mix.

Zero-test scene files this would give a home to: `water.rs` (638 lines — with
the pure `reconcile_region_planes:388` region-set diff and `water_params`),
`glow.rs` (619), `underwater_fog.rs` (427), `environment.rs` (328).
