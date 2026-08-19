---
id: viewer-camera-keyboard-controls
title: Keyboard camera controls — orbit / pan / zoom / roll actions
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-input-modifier-chords, viewer-camera-controls-window,
  viewer-camera-third-person-orbit, viewer-flycam-key-bindings-rethink,
  viewer-input-sitting-edit-avatar-modes]
---

Context: [context/viewer.md](../context/viewer.md).

The reference drives the third-person camera from the keyboard through
bindable actions: `spin_around_ccw/cw`, `spin_over/under`,
`move_forward/backward` (camera dolly, plus `_fast` variants),
`pan_up/down/left/right`, `pan_in/out`, and Firestorm's own
`roll_left/right` addition (default R/T). Default chords: Alt+arrows and
Alt+WASD orbit, Ctrl+Alt spins over/under (keyboard alt-zoom),
Ctrl+Alt+Shift pans. On top of that the menu exposes the zoom trio
Ctrl+0 / Ctrl+9 / Ctrl+8 (zoom in / default / out), Shift+Esc (reset
camera angles) and Ctrl+\ (look at last chatter).

Our third-person camera is mouse-only today
(`sl-client-bevy-viewer/src/camera.rs`, `orbit_third_person` around
lines 773-870: Alt+LMB drag plus wheel zoom); none of these camera
operations exist as `Action`s in
`sl-client-bevy-viewer/src/input_action.rs`, so they can never be bound
or rebound. Add them as a camera-family `Action` set resolved through
the per-mode binding profiles (the masked defaults need
[[viewer-input-modifier-chords]]) and drive the existing orbit/zoom rig
operations from them. The sitting and edit-avatar variants live in
[[viewer-input-sitting-edit-avatar-modes]]; the on-screen camera pad
([[viewer-camera-controls-window]]) should call the same operations so
keyboard, pad and mouse stay one rig. Whether we adopt Firestorm's bare
R/T roll defaults (safe there only because camera keys are mode-scoped)
is a call this task makes; a flycam default key is handled by
[[viewer-flycam-key-bindings-rethink]].

Reference (Firestorm, read-only): `indra/newview/llviewerinput.cpp`
(camera_* handlers), `indra/newview/app_settings/key_bindings.xml`
(third_person Alt / Ctrl+Alt blocks), `indra/newview/llagentcamera.cpp`
(setOrbit*/setPan* key plumbing).
