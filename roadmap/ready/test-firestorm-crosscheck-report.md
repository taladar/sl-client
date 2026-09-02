---
id: test-firestorm-crosscheck-report
title: Report the divergences — contact sheet, image diff, scene-dump diff
topic: test
status: ready
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

A difference that looked like a baseline turned out to be a real bug, and
the chase is worth keeping because the same shape of mistake will recur.
Firestorm drew every avatar here with **no right hand** — the forearm
ending in a torn edge at the wrist while the left hand was complete.

Everything about it pointed at this workspace and none of it was:

- not the grid — it reproduced against the live OpenSim as well;
- not the appearance — the agent's own avatar and a fixture NPC alike;
- not the bake — the served bakes decode to five planes with alpha and
  mask 255 everywhere;
- not the morphs — `LLHandMotion` reported the textbook resting state,
  and forcing every hand-pose morph to zero changed nothing;
- not LOD, not the graphics preset, not screen space (from behind, the
  gap follows the avatar's anatomical right hand).

The cause is upstream Linden code, `avatarSkinV.glsl`. Each vertex blends
between its joint and the *next* palette entry, and `setupJoint`
re-inserts `mChest` before each collar to keep consecutive entries a
parent→child pair — which fills the upper body's 15-slot palette exactly
and leaves `mWristRight` last, at index 14. Its third row's partner read
is `matrixPalette[45]`, one past the end of a 45-element array.

Both wrists bind **388 vertices at a blend of exactly zero** — mirror
images — so both compute `a*1 + b*0`. The left wrist's `b` is in bounds
and finite and contributes nothing; the right wrist's is out of bounds,
and `NaN * 0` is `NaN`. Those 388 vertices get NaN positions and their
triangles are dropped, so the hand *vanishes* rather than deforming. It
is driver-dependent, which is why it is not a defect every Firestorm user
has seen for a decade.

Reported upstream as [secondlife/viewer#6240][upstream] (2026-09-02); the
local Firestorm fork carries a fix that clamps the blend partner, which is
exact because it can only alter a blend already weighted zero.

[upstream]: https://github.com/secondlife/viewer/issues/6240

The lesson for this task: our viewer rendering the same avatar correctly
was the single most informative measurement, and it came late. When the
two viewers disagree, the one to suspect is not automatically ours —
"the reference viewer is right" is a prior, not evidence.

Note the calibration use in [[test-firestorm-fake-grid-crosscheck]] is
separate and still stands — that one records prose facts about what
Firestorm shows, and does not diff images.

Both blockers are cleared (2026-09-02): [[test-firestorm-crosscheck-runner]]
collects the runs and [[viewer-scene-dump]] gives this viewer the second
half of the pair. The first live pair of dumps settles some of this task's
design for it.

**The scene-dump diff must not key everything by id.** Three kinds of id
are minted locally and differ between two viewers of one scene — and
between two runs of one viewer:

- the reference's own viewer-side scene objects (256 `app-30` terrain
  patches plus sky, water and clouds, all `local_id` 0), which this viewer
  does not model as objects at all — 275 of Firestorm's 296 objects in the
  catalogue scene;
- control avatars, which have no grid identity: Firestorm minted
  `36a77dc9…` and then `ecce83a2…` for the *same* animesh in two
  consecutive runs. Our dump reports one by the animesh object it rides,
  flagged `is_control_avatar` — match on the flag and the object;
- baked avatar textures, whose ids are minted per bake by the client or the
  server, so the same appearance carries different ids on the two sides.

Match on grid ids only: object keys, mesh and sculpt assets, and ordinary
(non-baked) face textures. Everything else is compared by rule, or not at
all.

Two fields carry a **known semantic difference** rather than a divergence,
and the report should say so rather than rank them: `num_faces` is the
count this viewer drew and the count the reference declares, and
`is_flexible` is "declares itself flexible" here against "is being drawn
flexible" there.

And the first thing the pair actually found is not in the images at all:
the two viewers' `render` sections differ (`mesh_lod_boost` 1.0 against
2.0, draw distance 512 m against 128 m), which *explains* the `lod`
differences on five objects. A report that ranks scenes by pixel distance
without showing the render sections beside them hides its own answer.
See also [[viewer-camera-fov-parity]], which is a framing difference in
every frame until it is fixed.
