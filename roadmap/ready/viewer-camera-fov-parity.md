---
id: viewer-camera-fov-parity
title: The camera's field of view is 45° here and 60° in the reference viewer
topic: viewer
status: ready
origin: the first two-viewer cross-check run (2026-09-02)
points: 2
refs: [test-firestorm-crosscheck-runner, test-firestorm-crosscheck-report]
---

Context: [context/testing.md](../context/testing.md).

The first run of [[test-firestorm-crosscheck-runner]] put both viewers at
exactly the same camera — Firestorm's own `scene.json` reports
`origin_region [124, 128, 27.5]`, `focus_region [124, 136, 25.5]`, the
values the runner asked for — and the two frames still show different
amounts of the scene. Firestorm frames five prims of the catalogue row
where we frame three.

The cause is not the pose but the lens. `viewer_projection()`
(`sl-viewer-world-scene/src/viewer_camera.rs`) takes Bevy's default **45°**
vertical field of view; Firestorm's dump says `fov_radians 1.0472`, which
is **60°**. The far plane differs by more: 4096 m here against its
`far_clip 128` (its draw distance), which changes where fog and the horizon
land as well.

Two separate things follow, and they should not be conflated.

**The fidelity one, which is the real bug.** The reference viewer's default
vertical FOV is what a Second Life user's muscle memory is calibrated to —
how far away things look, how much of a room fits on screen, how a camera
offset feels. Ours being 15° narrower is a difference every user would
notice against every other viewer, in every session, quite apart from any
harness. Check what the reference actually defaults to (`CameraAngle` in
`settings.xml`, and how `LLViewerCamera` derives the view from it,
including the aspect-dependent clamping) rather than taking 60° from one
dump, then match it — and while there, decide whether the far plane should
follow the draw distance as it does there.

**The harness one.** Even once the defaults agree, a cross-check must not
*rely* on two viewers' defaults agreeing: that is a comparison whose
premise is unstated and silently breaks on the next settings change. The
capture block should carry the lens as it carries the size — an
`SL_VIEWER_CAPTURE_FOV` (and the far plane with it) that both harnesses
apply — so a run's frames are the same view by construction, and
`sl-crosscheck` records what it asked for in `run.json`. The Firestorm half
is a change in the fork, beside its existing `--camera-position`.

Until then, a cross-check frame pair differs in framing everywhere, and
[[test-firestorm-crosscheck-report]] would rank that as the largest
divergence in every scene — which is exactly the "expect a large baseline
difference and say so" case that task warns about, except that this one is
fixable rather than inherent.
