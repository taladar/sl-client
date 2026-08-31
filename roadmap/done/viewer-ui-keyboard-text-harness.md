---
id: viewer-ui-keyboard-text-harness
title: Keyboard focus, bindings and text entry, headless
topic: viewer
status: done
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

The unknown, resolved (2026-08-31): **`bevy_text`'s `TextPlugin`** owns
`EditableTextSystems` and its one system, `apply_text_edits`, and it drags
in **no renderer** — a `Font` asset store, a `ClipboardPlugin` (an
in-process buffer without the `system_clipboard` feature) and parley's
contexts, all of which the harness already had. The catch is that the
*UI* half of editing is not in that plugin at all: it lives in `bevy_ui`'s
`build_text_interop`, a private function of `UiPlugin`, which puts
`update_editable_text_content_size` / `_styles` before the set and
`update_editable_text_layout` / `scroll_editable_text` after layout. All
four are `pub`, so `interact::install_text_editing` picks them the way the
harness already picks `ui_layout_system`. The last of them shapes glyphs
into a `FontAtlasSet` backed by `Assets<Image>` — CPU images in an asset
store, not a GPU dependency. Content size is the load-bearing one: a
field's box is its *editor's* intrinsic size (`visible_width` in `"0"`
advances), not a measured `Text` node's, so without it every field lays
out at zero and the pointer has nothing to hit.

Landed (2026-08-31), the drivers: `install_text_editing` (called from
`InteractionTest::build`), the four `ime_*` drivers writing `Ime` plus its
`WindowEvent` wrapper, `focus` / `blur` / `text_of`, `with_modifier` for
chords, and a `type_str` that now presses the **physical** key the
character sits on (`key_code_for`) instead of one placeholder key code.
That last one is the tier's quiet trap: the field reads only the *logical*
key, so a placeholder would drive text entry perfectly while telling
`ButtonInput<KeyCode>` — which every world binding profile reads — that no
letter had been pressed, and a focus-routing test built on it would be
asserting a coincidence. Five teeth in `interact.rs`.

Landed (2026-08-31), the consumers, each beside the code it pins:

- `ui_text_input.rs::typed_tests` — the filters and the validators *wired*
  rather than called: a letter refused by the `EditableTextFilter` before
  it enters the buffer, a second `.` and a trailing `-` reverted by the
  structural pass, an unsigned field with no sign key, `max_characters`,
  `Enter` as a newline only where newlines are allowed, the `Insert` key
  toggling overwrite (and inert with nothing focused), and a disabled
  field taking no input at all.
- `ui_search.rs` / `chat_input.rs` — the cancel and commit gestures typed:
  `Escape` clears the *focused* search field and no other, and a chat line
  typed on a real keyboard is sent by a real `Enter` carrying its `Shift`.
  Both existing tests set the text through `set_text` and poke
  `ButtonInput`, so neither could see that `Enter` reaches the sender
  **only because** a single-line field refuses newlines and lets the key
  propagate.
- `input_context.rs::typed_tests` / `input_action.rs::typed_tests` — the
  composition the hand-poked tests structurally cannot reach: `W` typed
  into a focused field is the field's letter and never `MoveForward`,
  `Escape` hands the keyboard back and the same key then walks, an
  **arrow** key moves the caret and not the avatar (the case the module
  documentation singles out, and the one a "is this a character key" gate
  would pass the letter test and fail), the arrow resolving per
  `InputMode`, and `Shift+W` resolving to `Run` *and* `MoveForward`
  together.
