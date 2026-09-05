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

- ``Ctrl+` `` — quick snapshot to disk (`snapshot_floater.rs`,
  [[viewer-snapshot-quick-key]]).
- The edit-drag `Ctrl` / `Ctrl+Shift` rotate / stretch modifiers
  (`edit_tool.rs`).

These work, but they are exactly the hardcoded keys the action map exists to
replace, and none of them is **rebindable** — the rebinding UI
([[viewer-input-rebinding-ui]]) cannot see or change a key that never enters a
`BindingProfile`.

**Shorter than it was** (2026-09-05): every chord that is *drawn on a menu
entry* — `Ctrl+L` / `Ctrl+Shift+L`, `Ctrl+B`, `Ctrl+Z` / `Ctrl+Y`, `Ctrl+I`,
`Ctrl+M`, `Ctrl+Q`, `Ctrl+Alt+Shift+S` — no longer has a bespoke handler at all.
[[viewer-menu-accelerators-inert]] made a `MenuCommand`'s `accel` label the
binding: `sl-viewer-ui-widgets`' `menu_accel` parses it and routes the chord to
the entry, honouring its `enabled_when`. So this task's migration list is the
chords with *no* menu entry, and its remaining argument is **rebindability** —
a menu accelerator is still authored in a `static`, and the rebinding UI
([[viewer-input-rebinding-ui]]) cannot see it. Whatever this map grows should
therefore feed the menu's drawn label rather than compete with it, or the two
disagree again in the other direction.

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

## Parity-audit addendum (2026-08-19)

The parity audit found that several already-implemented floaters and
features lack their reference default chord entirely (no accelerator
drawn, no handler). Once chords exist in the action map, add these
defaults: Ctrl+Shift+M Mini-Map, Ctrl+Shift+A Nearby people /
Ctrl+Shift+F Friends / Ctrl+Shift+G Groups (people-panel tabs),
Ctrl+Shift+S Snapshot floater (only Ctrl+` snapshot-to-disk exists
today), Ctrl+Shift+W Close All Windows and Ctrl+Alt+W Close Window
Group (the floater manager has only Ctrl+W close-one), and Ctrl+. /
Ctrl+, select-next/previous part-or-face (edit-face selection is done;
its cycling keys can join the chord migration). The dead drawn
accelerators (Ctrl+P/T/F/U) were the separate bug
[[viewer-menu-accelerators-inert]], now fixed: its generic accel→command
dispatch is what these defaults should be authored into — an entry with an
`accel` label is a live chord, so several of the above need no map work at all,
only a menu entry to hang on.
