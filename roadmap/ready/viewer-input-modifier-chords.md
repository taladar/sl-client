---
id: viewer-input-modifier-chords
title: Modifier-chord bindings in the input action map
topic: viewer
status: ready
origin: split from viewer-snapshot-quick-key (2026-07) — Ctrl+` needed a chord
blocked_by: [viewer-input-action-map]
refs: [viewer-input-rebinding-ui, viewer-snapshot-quick-key, viewer-input-gesture-bindings]
---

Context: [context/viewer.md](../context/viewer.md).

Extend the input action map ([[viewer-input-action-map]]) so a binding can
require **modifier keys** (Ctrl / Shift / Alt), not just a bare key. Today the
map keys on a single `KeyCode`, so every viewer **chord** is instead a
**hardcoded** direct-keyboard handler, each re-checking the modifiers and the
world-keyboard gate by hand:

- `Ctrl+L` / `Ctrl+Shift+L` — link / unlink (`edit_link.rs`).
- `Ctrl+B` — toggle the Build Tools floater (`edit_tool.rs`).
- ``Ctrl+` `` — quick snapshot to disk (`snapshot_floater.rs`,
  [[viewer-snapshot-quick-key]]).
- The edit-drag `Ctrl` / `Ctrl+Shift` rotate / stretch modifiers
  (`edit_tool.rs`).

These work, but they are exactly the hardcoded keys the action map exists to
replace, and none of them is **rebindable** — the rebinding UI
([[viewer-input-rebinding-ui]]) cannot see or change a key that never enters a
`BindingProfile`.

Scope: give a binding an optional required-modifier set (a
`KeyChord { key, ctrl, shift, alt }` with `From<KeyCode>` so the existing
single-key builders are unchanged), and match it in `update_action_input` with
the non-exclusive rule that already fits the movement keys — a binding fires
when its key is pressed **and** its required modifiers are held, without
forbidding *extra* modifiers (so bare `W` still walks while `Ctrl` is held, but
``Ctrl+` `` only fires with `Ctrl`). Then migrate the hardcoded chords above
onto `Action` variants bound in the profiles, and delete their bespoke handlers.

Reference (Firestorm, read-only): `indra/newview/llviewerinput.cpp` and
`keys.xml` (whose bindings carry a `mask` of modifier bits).

Builds on: [[viewer-input-action-map]] (the single-key map this widens);
unblocks the chords in [[viewer-input-rebinding-ui]].
