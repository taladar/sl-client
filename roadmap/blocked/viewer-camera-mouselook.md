---
id: viewer-camera-mouselook
title: Mouselook (first-person) camera
topic: viewer
status: blocked
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
