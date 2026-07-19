---
id: viewer-input-keyboard-layout-bindings
title: Keyboard bindings stable across physical keyboard layouts
topic: viewer
status: ready
origin: raised during viewer-i18n-fluent-scaffold (2026-07)
blocked_by: [viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

The input action map ([[viewer-input-action-map]]) binds named actions to keys.
This task makes those bindings behave correctly for users whose physical
keyboard layout is not US-QWERTY (AZERTY in France, QWERTZ in Germany, Dvorak,
Colemak, JIS, …), which is a distinct concern from string i18n but the same
"do not assume the author's locale" family.

The core distinction is **physical position vs. printed label**, and different
bindings want different answers:

- **Positional bindings** — movement (WASD), the ones chosen for where the keys
  *sit* under the hand. On AZERTY the `W`/`A` positions carry `Z`/`Q`, so a
  binding stored by *logical key* would scatter the movement cluster. These
  should bind by **physical scancode** (Bevy's `KeyCode`, which is already
  position-based and layout-independent) so `W` stays under the same finger
  everywhere. Confirm the current map genuinely stores `KeyCode` (position),
  not a logical key, and that the settings UI presents them as positions.
- **Mnemonic bindings** — shortcuts chosen for the *letter* (`M` for map, `I`
  for inventory, `B` for build). A user on QWERTZ expects the key **labelled**
  `M`, wherever it physically sits. These want the **logical key** (Bevy's
  `Key`, resolved through the OS layout), not the scancode. Bevy 0.19 delivers
  both on `KeyboardInput`; the action map needs to record, per binding, which
  semantics it wants.

Deliverables:

- A per-binding **position vs. label** flag on the action map, with a sane
  default per action class (movement = position, mnemonic = label).
- Resolve label bindings through the live OS layout (dead keys, `AltGr`
  layers, and non-Latin layouts where the printed label is not an ASCII
  letter — e.g. a Cyrillic or Greek layout — which means a mnemonic may have no
  reachable key and needs a fallback).
- The rebinding UI shows the label the user sees, and round-trips through a
  settings store ([[viewer-ui-settings-store]]) in a layout-stable form.
- Tests for the AZERTY/QWERTZ/Dvorak remaps of the default profile.

Reference (Firestorm, read-only): `LLKeyboard` / `LLKeyboardSDL` (scancode →
key translation), `llviewerinput` and the `key_bindings` settings, and how the
reference viewer keeps WASD positional while letter shortcuts follow the layout.
