---
id: viewer-camera-mouselook
title: Mouselook (first-person) camera
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-camera-third-person-orbit, viewer-input-focus-contexts]
---

Context: [context/viewer.md](../context/viewer.md).

First-person **mouselook**: the camera sits at the avatar's eyes, mouse motion
aims, and the cursor is captured. Tied to the **Mouselook** input context
([[viewer-input-focus-contexts]]) so its own binding profile is live (the
mouselook `keys.xml` mode) and the cursor is grabbed only while in it.

Reference (Firestorm, read-only): `llagentcamera` first-person handling,
`lltoolfocus`, `llviewerwindow` cursor capture.

## Done

`src/camera.rs`. Mouselook sits at the avatar's head (`mHead` joint, correct
even when sitting), nudged forward past the face; the mouse aims and the cursor
is captured (`crate::input_context` grabs the pointer in mouselook and nowhere
else). Entered with `M` or by zooming third person in past the minimum distance;
left by `M`, scrolling out, or `Escape` (reset view). The eye position is
low-passed to filter the animated head-joint micro-motion while the aim stays
responsive. The avatar body heading follows the camera aim (published via
`CameraAim`). `Escape` resets to the default rear view rather than quitting;
quit moved to `Ctrl+Q` (reference). The keys.xml mouselook mode is the
`InputMode::Mouselook` profile in [[viewer-input-action-map]].
