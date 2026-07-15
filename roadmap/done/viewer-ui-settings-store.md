---
id: viewer-ui-settings-store
title: Typed persistent settings store
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui/viewer-ui-framework
refs: [viewer-ui-settings-binding]
---

Context: [context/viewer.md](../context/viewer.md).

A pure, typed, persistent settings store: named settings with types and sensible
defaults, load/save to disk, and per-account overrides layered over the global
defaults. No UI — this is the backend that many later tasks read and write:
input rebinding ([[viewer-input-rebinding-persistence]]), camera presets
([[viewer-camera-presets]]), i18n locale ([[viewer-i18n-locale-selection]]), the
SpaceNavigator axis mapping ([[viewer-input-spacenav-camera-mapping]]), and the
chat auto-open-on-typing toggle ([[viewer-chat-input-bar]]) all consume it.

The two-way widget binding on top of this store is a separate task
([[viewer-ui-settings-binding]]); this one owns only the store + persistence.
Model it after the reference's design-token indirection and `control_name=`
two-way binding (1,293 uses — the reason ~20 preference panels have almost no
code behind them), which is why the store is a first-class shared resource
rather than scattered per-panel state.

**Do not copy** the reference's `LLInitParam` (2,000 lines of C++ templates
reimplementing serde) — use serde.

Reference (Firestorm, read-only): `llviewercontrol` (the settings backend),
`llcontrolgroup`, `llfloatersettingsdebug` (the raw debug settings editor).

## Done

New pure crate **`sl-settings`** (Bevy-free; `serde` + `serde_json` + `fs_err` +
`thiserror`), unit-tested (`cargo test -p sl-settings`, 12 tests + doctest,
clippy/fmt/doc-clean). Shape:

- **`SettingValue`** — a tagged value enum covering the reference control types
  used for stored preferences: `Bool`, `I32` (`TYPE_S32`), `U32`, `F32`,
  `String`, `Color3`/`Color4` (linear `f32` channels), `Vec3`/`Vec3d`, `Rect`
  (`[left, top, right, bottom]`). The reference `TYPE_LLSD` escape hatch is
  deliberately omitted — a value is always one concrete typed shape.
  `SettingKind` is the matching type tag used for write/read type-checking.
- **`SettingsStore`** — `register`/`register_transient` declare a setting once
  with a typed default + comment (duplicate name → error). Two override
  `Scope`s (`Global`, `Account`) layer over the declared defaults; the effective
  value resolves **account → global → default**. `set` type-checks against the
  declaration; `reset` drops one override; `clear_scope` drops a whole layer
  (account-on-logout). Typed getters (`get_bool`/`get_f32`/…) fail with
  `TypeMismatch`; `get`/`names`/`declaration` expose the generic view the raw
  debug settings editor ([[viewer-ui-settings-binding]] and a future editor)
  will iterate.
- **Persistence** — `save_scope`/`load_scope` per scope to an independent JSON
  file (name-keyed adjacently-tagged map), so the global file is shared while a
  per-account file is swapped per login. A missing file loads as a no-op;
  transient overrides are skipped on save; on load a type-changed entry is
  dropped while a not-yet-registered entry is kept (forward-compat round-trip).

**Modelled on serde, not the reference `LLInitParam`** as directed. Scope kept
to the store + persistence only — the Bevy `Resource` wrapper and two-way widget
binding are [[viewer-ui-settings-binding]]; no viewer wiring added yet (no
consumers until that task), so the crate has no reverse-dependency yet and is
verified purely by its own tests. The task's "per-account overrides layered over
the global defaults" is implemented as the three-layer account→global→default
resolution above (a small generalization of the reference's two disjoint control
groups, matching the task's layering wording).
