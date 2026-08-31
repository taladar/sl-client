---
id: viewer-ui-keyboard-text-harness
title: Keyboard focus, bindings and text entry, headless
topic: viewer
status: ready
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 5
blocked_by: [viewer-ui-interaction-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The keyboard has two viewer layers — `input_context.rs` (`InputContext`
derived from `InputFocus`, the `world_has_keyboard` run-condition) and
`input_action.rs` (`ButtonInput<Action>` rebuilt from `InputBindings` per
`InputMode`) — plus `bevy::text::EditableText` editing via
`bevy_ui_widgets`' `EditableTextInputPlugin` (`KeyboardInput` + `Ime`
messages; parley editing is CPU-side).

Extend the interaction harness with `type_str`/IME drivers and stand the
editable-text path up headlessly. Assert:

- focus routing — `world_has_keyboard` vs a focused `TextEntry` swallowing
  movement keys;
- the `TextInputKind` filters/parsers in `ui_text_input.rs` under real
  keystrokes (garbage rejected, overwrite mode, per-char filters);
- Escape/Enter commit-and-cancel semantics;
- `Action` chord resolution per `InputMode`.

The one real unknown to establish first: which plugin registers
`EditableTextSystems` and whether it drags in render dependencies.
