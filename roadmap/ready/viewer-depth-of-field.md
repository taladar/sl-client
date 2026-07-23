---
id: viewer-depth-of-field
title: Depth of field
topic: viewer
status: ready
origin: render-feature gap analysis vs Firestorm (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The lens-focus blur that is half of what makes an SL photo look like a
photograph — sharp at the focal plane, blurred in front of and behind it. A
post-process pass on the deferred pipeline, driven by a physical-ish camera
model, and one of the features [[viewer-phototools]] exists to expose.

Firestorm gates it on `RenderDepthOfField` and drives it from camera params:
`CameraFNumber` (aperture — how shallow the focus), `CameraFocalLength`,
`CameraFieldOfView`, `CameraMaxCoF` (circle-of-confusion cap),
`CameraFocusTransitionTime` (focus-pull easing) and `CameraDoFResScale`. Focus
point is picked from the camera's focus target, with a follow-the-pointer option
(`FSFocusPointFollowsPointer`).

Scope: a circle-of-confusion computed from the depth buffer and the aperture
model, a bokeh blur (gather or separable), the focus-transition easing so
pulling focus is smooth, and the camera-parameter controls. Relates to
[[viewer-camera-third-person-orbit]] for the focus target and
[[viewer-phototools]] for the knobs. A cheap sibling VFX post the reference also
has — **vignette** (`FSRenderVignette`: amount / power / multiplier, a shader
with no stock UI) — is worth folding in here rather than its own task.

Also itemize the FS focus conveniences (main-menu survey 2026-07-23):
**Focus Lock** (World ▸ Photo and Video, Alt+Shift+X — pin the focus
distance so recomposing doesn't refocus) and the **DoF focus crosshair**
toggle (draw the current focus point on screen).

Reference (Firestorm, read-only): the deferred DoF post pass,
`RenderDepthOfField` and the `Camera*` settings.

Builds on: the existing deferred pipeline and depth buffer.
