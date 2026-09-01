---
id: test-firestorm-crosscheck-report
title: Report the divergences — contact sheet, image diff, scene-dump diff
topic: test
status: blocked
origin: Firestorm cross-check harness plan (2026-09-01)
points: 5
refs: [test-firestorm-fake-grid-crosscheck]
blocked_by: [test-firestorm-crosscheck-runner, viewer-scene-dump]
---

Context: [context/testing.md](../context/testing.md).

Turn a collected run into something a person can act on. Three outputs,
in increasing order of how often they actually identify the bug:

- **A side-by-side contact sheet** — the two viewers' frames for each
  scene, tiled, with the scenario and camera named. This is what gets
  looked at first and what gets pasted into an issue.
- **An image diff** — per-pixel or SSIM with a tolerance, to rank scenes
  by how far apart they are so attention goes to the worst one. A number,
  not a verdict.
- **A scene-dump diff** — the structured comparison over `scene.json`.
  Compare by object id, report objects present in one dump and not the
  other, then per-field differences with a tolerance on floats. This is
  the output that names the cause: a texture id that differs, a LOD that
  differs, a material missing on one side.

**This is a developer-facing tool, not a CI gate.** It never enters
`cargo nextest` and never fails a build. The reason is the same one behind
this workspace's no-golden-images rule: a pixel comparison across two
renderers, two GPUs and two driver versions measures the environment at
least as much as the code, and a check that fails on a Mesa upgrade is one
that gets disabled and then ignored. The tiered harness stays the thing
that says *wrong*; this says *different*, and a human decides which
viewer is right.

Expect a large baseline difference and say so in the report rather than
treating it as a finding: the two viewers do not share a renderer, so
tone mapping, exposure, shadow filtering and anti-aliasing differ
everywhere at once. The signal is a *change* in the difference, or a
difference localised to one object.

Note the calibration use in [[test-firestorm-fake-grid-crosscheck]] is
separate and still stands — that one records prose facts about what
Firestorm shows, and does not diff images.
