---
id: viewer-input-sitting-edit-avatar-modes
title: Sitting & edit-avatar input modes with their binding profiles
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-camera-keyboard-controls, viewer-sit-stand-actions,
  viewer-input-script-control-capture, viewer-appearance-editor-shell]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's `key_bindings.xml` has four binding modes; our
`InputMode` (`sl-client-bevy-viewer/src/input_context.rs`) has only
ThirdPerson, Mouselook and Flycam equivalents. Missing are `sitting`
and `edit_avatar`, each with its own default profile and 12 dedicated
actions: `spin_around_ccw/cw_sitting`, `spin_over/under_sitting`,
`move_forward/backward_sitting`, and the `edit_avatar_spin_ccw/cw/
over/under` plus `edit_avatar_move_forward/backward` set.

Sitting: while seated, the reference's default keys become camera
actions that orbit the camera around the seat — but each handler falls
back to sending the agent control bits when a script has grabbed that
control, the sit camera is scripted, or the agent is running
(`llviewerinput.cpp`, `camera_spin_around_ccw_sitting` and friends),
and the Shift variants always pass controls to the vehicle. Ours
hardwires the seated branch to vehicle steering:
`sl-client-bevy-viewer/src/movement.rs` (around lines 436-448) always
emits YAW control bits when `agent.seated_on.is_some()`, so sitting on
an ordinary chair turns the movement keys into dead keys instead of
orbiting the camera. EditAvatar: while the appearance editor
([[viewer-appearance-editor-shell]], done) is open, the reference
rebinds WASD/arrows to orbit the camera around the own avatar; ours
leaves the world bindings untouched.

Implement the two `InputMode`s (derived from SitState and the
appearance-edit state), their default `BindingProfile`s, and the
grabbed-control fallback rule (which needs the control-grab knowledge
of [[viewer-input-script-control-capture]]). The camera actions
themselves come from [[viewer-camera-keyboard-controls]]; registering
`toggle_sit` as a bindable action travels with
[[viewer-sit-stand-actions]].

Reference (Firestorm, read-only):
`indra/newview/app_settings/key_bindings.xml` (sitting, edit_avatar
modes), `indra/newview/llviewerinput.cpp` (…_sitting and edit_avatar_*
handlers).
