---
id: viewer-input-locomotion-actions
title: Missing locomotion / utility bindable actions (stop, run, strafe, look)
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-input-modifier-chords, viewer-movement-controls-floater,
  viewer-qol-toggles, viewer-takeoff-hold-jumps-instead-of-flying,
  viewer-media-prim-browser, viewer-streaming-audio]
---

Context: [context/viewer.md](../context/viewer.md).

Our `Action` set in `sl-client-bevy-viewer/src/input_action.rs` covers
10 of the reference's roughly 20 avatar-scope actions. Missing:

`stop_moving` (default Space — zero all motion and stop the autopilot);
`toggle_run` (menu Ctrl+R "Always Run" — the protocol side,
sl-proto's `Command::SetAlwaysRun` at `sl-proto/src/command.rs:1878`,
exists and is never issued by the viewer; today Run is only a held
Shift modifier / double-tap latch in
`sl-client-bevy-viewer/src/movement.rs` around lines 403-424) together
with the `run_forward/backward/left/right` directionals; the
third-person strafe pair `slide_left/right` (default Shift+A/D and
Shift+arrows — we strafe only in mouselook, third person has no strafe
at all); `look_up`/`look_down` (bindable mouselook pitch — pitch is
mouse-only today); and the two media actions `toggle_pause_media` /
`toggle_enable_media` (ours has only a per-surface pause button in
`sl-client-bevy-viewer/src/media_controls.rs`, relevant to
[[viewer-media-prim-browser]] and [[viewer-streaming-audio]]). Also add
the reference's missing default binding Home → `toggle_fly`.

This task is the actions, their `movement.rs` semantics and the default
keys. The Shift-masked defaults need [[viewer-input-modifier-chords]];
the Always-Run UI surface stays with
[[viewer-movement-controls-floater]] and [[viewer-qol-toggles]]. The
existing hold-to-fly regression is tracked separately as
[[viewer-takeoff-hold-jumps-instead-of-flying]].

Reference (Firestorm, read-only): `indra/newview/llviewerinput.cpp`
(agent_slide_*, run_*, toggle_run, stop_moving, agent_look_up/down,
toggle_pause_media),
`indra/newview/app_settings/key_bindings.xml`.
