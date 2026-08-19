---
id: viewer-input-mouse-button-bindings
title: Mouse buttons as binding sources in the input action map
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-input-modifier-chords, viewer-input-rebinding-ui,
  viewer-voice-controls, viewer-input-script-control-capture]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's binding table is not keyboard-only. `key_bindings.xml`
binds `mouse="MMB"` → `toggle_voice` (push-to-talk) and `mouse="LMB"` →
`script_trigger_lbutton` in every one of its four modes, and the rebind
UI's key-capture floater (`floater_select_key.xml`) captures any mouse
button — including button4/5 and double-click — with modifier masks, so
users can put actions on spare mouse buttons.

Our `BindingProfile` in `sl-client-bevy-viewer/src/input_action.rs`
maps `KeyCode → BindingTarget` only; no mouse button can ever be bound
to an action. That blocks the default MMB push-to-talk of
[[viewer-voice-controls]] and the mouselook-LMB script trigger of
[[viewer-input-script-control-capture]]. Widen the binding source to a
key-or-mouse-button enum (composing with the chord masks of
[[viewer-input-modifier-chords]]), resolve mouse-button bindings in
`update_action_input` under the same focus gate (a click on UI must not
fire a world action), and expose mouse capture in the rebinding UI's
capture flow ([[viewer-input-rebinding-ui]]).

Reference (Firestorm, read-only):
`indra/newview/app_settings/key_bindings.xml` (mouse= attributes),
`indra/newview/llviewerinput.cpp`/`.h` (EMouseClickType in
LLKeyboardBinding),
`indra/newview/skins/default/xui/en/floater_select_key.xml`.
