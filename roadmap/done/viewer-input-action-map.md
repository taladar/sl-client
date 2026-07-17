---
id: viewer-input-action-map
title: Input action map & per-context binding profiles
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-input-system
blocked_by: [viewer-input-focus-contexts]
---

Context: [context/viewer.md](../context/viewer.md).

A real input-mapping layer replacing today's hardcoded keys: **named actions**
and a **per-context binding-profile system**. Each input context
([[viewer-input-focus-contexts]]) owns its own action→binding map, so one
physical key means different things in mouselook vs. third-person vs. sitting —
mirroring the per-mode blocks in Firestorm's `keys.xml`. Resolution walks the
active context's profile and emits action events; ship sensible default
profiles; and replace the hardcoded keys in `movement.rs` / `camera.rs`.

Two structural requirements:

- **Many-to-one bindings.** An action may have several bindings at once (e.g.
  both `W` and `↑` → forward). The binding→action map is many-to-one.
- **Dynamic binding targets.** The target of a binding is not a closed action
  enum — it may be a **dynamic entry**, notably a bound inventory **gesture**
  ([[viewer-input-gesture-bindings]]). Design the target model open from the
  start.

Bevy has no canonical name for this; model it ourselves as context-keyed binding
profiles (cf. leafwing per-context `InputMap`, `bevy_enhanced_input`
`InputContext`). Persistence is [[viewer-input-rebinding-persistence]]; the
editor UI is [[viewer-input-rebinding-ui]].

Reference (Firestorm, read-only): `llviewerinput.cpp/h` (the keybinding table,
`keys.xml`), `indra/llwindow/llkeyboard`.

Builds on: `movement.rs` / `camera.rs` (currently fixed keys).

## Done

`src/input_action.rs`. Named [`Action`] set + per-[`InputMode`]
`BindingProfile`s in an `InputBindings` resource (reference-faithful default
keys), resolved each frame into a `ButtonInput<Action>` so `movement.rs` /
`camera.rs` read actions with the same `pressed` / `just_pressed` interface they
read keys with. The focus-gate lives once in the resolver (a focused UI zeroes
every action), replacing the per-system `world_has_keyboard` run-conditions on
movement / camera. Many-to-one bindings fall out of the `key -> target` map; the
dynamic target model is `BindingTarget` (an open enum whose `Gesture` arm the
gesture-binding task fills). The keys.xml **mode** axis this introduces
(`InputMode`, derived from the camera mode) is the second value
`viewer-input-focus-contexts` deferred until mouselook / flycam existed. The
rebinding editor / persistence / gesture targets stay their own tasks.
