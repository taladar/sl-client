# sl-settings

A typed, persistent settings store for a Second Life / OpenSim viewer. Settings
are named, given a typed default up front, then read and written by name and
persisted to disk. It is the backend the preference UI binds to — the two-way
widget binding on top of it is a separate concern.

It is the Rust counterpart of the reference viewer's `LLControlGroup` /
`llviewercontrol` (the global `gSavedSettings` plus the per-account
`gSavedPerAccountSettings`), but built on `serde` rather than the reference's
hand-rolled `LLInitParam` control serialization.

## Model

- **Declarations** — [`SettingsStore::register`] declares a setting once with a
  typed default ([`SettingValue`]) and a comment. The default's type fixes the
  setting's type: a later write of a different type is rejected. A transient
  (runtime-only) setting is declared with
  [`register_transient`](SettingsStore::register_transient) so its overrides are
  never written to disk.
- **Layers** — two override [`Scope`]s sit over the declared defaults: a
  machine-wide `Global` layer shared by every account, and an `Account` layer
  loaded when an account logs in and cleared on logout. The effective value of a
  setting resolves **account → global → default**.
- **Typed access** — [`get_bool`](SettingsStore::get_bool),
  [`get_f32`](SettingsStore::get_f32), … read the effective value and fail with
  a [`SettingError::TypeMismatch`] if used on the wrong type;
  [`set`](SettingsStore::set) writes one scope. A raw settings editor can
  enumerate every setting with [`names`](SettingsStore::names) and read them
  generically with [`get`](SettingsStore::get).

## Persistence

Each scope saves to and loads from an independent JSON file
([`save_scope`](SettingsStore::save_scope) /
[`load_scope`](SettingsStore::load_scope)), so the global file is shared while a
per-account file is swapped on login. Only overridden, persistable settings are
written; the on-disk shape is a name-keyed map of adjacently-tagged values:

```json
{
  "RenderFarClip": { "type": "f32", "value": 128.0 },
  "ShowChatBubbles": { "type": "bool", "value": true }
}
```

A missing file loads as a no-op (first run). On load, an entry whose type no
longer matches its declaration (a type changed across versions) is dropped,
while an entry for a not-yet-registered setting is kept, so a value written by a
newer build is not lost on round-trip.

## Value types

[`SettingValue`] covers the control types the reference viewer uses for stored
preferences: `Bool`, `I32`, `U32`, `F32`, `String`, `Color3` / `Color4` (linear
`f32` channels), `Vec3` / `Vec3d`, and `Rect` (`[left, top, right, bottom]`).
