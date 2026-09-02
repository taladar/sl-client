---
id: viewer-camera-fov-parity
title: The camera's field of view is 45° here and 60° in the reference viewer
topic: viewer
status: done
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

Done (2026-09-02). The reference's number is **60°**: `CameraAngle`
defaults to `1.047197551` and `DEFAULT_FIELD_OF_VIEW` is `60.f * DEG_TO_RAD`
(`llcamera.h`). Ours is now that, from one constant in
`sl-viewer-world-scene/src/viewer_camera.rs` that `viewer_projection()` and
the `CameraAngle` default both read — so the render tier's CPU projection
oracle moves with the camera instead of drifting from it.

The mechanism was already there, which is the part worth remembering: the
`CameraAngle` setting, its preferences slider and an `apply_camera_fov`
system all existed, and the setting's registered default was **deliberately**
Bevy's 45°, with a comment saying so ("The reference defaults wider (1.047 =
60°); ours keeps the out-of-the-box view"). Nothing was missing; a decision
had been taken to keep the framework's framing, and nothing could see what it
cost until two viewers photographed one scene from one pose.

The clamp came with it. Ours was a flat 5°–175°; the reference's is
**aspect-dependent** (`LLCamera::getMinView` / `getMaxView`), because those
bounds limit the *horizontal* extent — a wide view therefore admits a
narrower vertical field than a square one, and without the scaling a 21:9
window opens past 175° horizontally and the projection turns inside out at
the edges. `clamp_field_of_view` ports it and `apply_camera_fov` applies it
against each view's own aspect.

The far plane is deliberately **not** matched, and the reason is in the
projection's doc comment. The reference sets its far clip to the draw
distance (`setFar(mDrawDistance)`) and draws its sky in a pass the clip never
reaches; ours is one scene whose sky dome is 3 km out and whose cloud dome is
15 km, so a far plane at the draw distance would clip the sky away entirely.
Matching it there is a change to how the sky is drawn, not to a number.

The harness half shipped too, in both viewers: `--camera-fov <degrees>` /
`SL_VIEWER_CAPTURE_FOV` here (through a run-scoped `CameraFovOverride`
resource, so a pinned lens never rewrites the operator's preferences), the
same variable in the Firestorm fork (applied through `setDefaultFOV`, which
does the reference's own clamping, and forced non-persistently because
`CameraAngle` is `Persist=1`), and `sl-crosscheck --fov` putting it in the
shared capture block. A run no longer *needs* it — the defaults agree — but
a comparison whose framing rests on two defaults agreeing is one with an
unstated premise, and that premise is exactly what hid this.

Verified two ways. Both viewers' scene dumps report `fov_radians 1.047198`
from the identical camera origin, and the frames agree: the same five
subjects of the catalogue row at the same screen positions, where ours
framed three. Two render baselines moved and were re-blessed — the tree and
the terrain patch, whose off-centre framing points moved *toward* the frame
centre, which is what a wider lens does; every centred subject stayed
centred.

One thing not to misread in a dump: the two `camera.aspect` values still
differ (1.778 here, 1.388 there). Firestorm's snapshot renders at the
capture's aspect but its `LLViewerCamera` reports the *window's* by the time
the dump is taken, so the frames match while the field says otherwise.
Noted in [[test-firestorm-crosscheck-report]].
