---
id: viewer-settings-account-scope-persist
title: Load and save both global and per-account settings in the viewer
topic: viewer
status: ready
origin: noticed wiring viewer-settings-toml-format (2026-07)
blocked_by: [viewer-ui-settings-store]
refs: [viewer-settings-toml-format]
---

Context: [context/viewer.md](../context/viewer.md).

The settings store ([[viewer-ui-settings-store]]) already models the split the
reference viewer has: a machine-wide `Global` layer and a per-account `Account`
layer that resolves over it (`account → global → default`), each persisted to
its own file
via `save_scope` / `load_scope`. But the viewer
(`sl-client-bevy-viewer/src/settings.rs`) currently wires only the **Global**
scope (`viewer-settings.toml`). Nothing loads a per-account file on login or
saves it on logout, so a setting a user wants tuned for *one* avatar (and not
globally) has nowhere to live.

Wire the **Account** scope end-to-end:

- On login, once the avatar identity is known, `load_scope(Scope::Account, …)`
  from that avatar's file; on logout, `clear_scope(Scope::Account)` (and save it
  first, mirroring `save_settings_on_logout`).
- **Key the account file by grid + avatar name together**, not name alone. The
  same avatar name on OpenSim vs Agni vs Aditi is **three different avatars**
  and must get three different account files — keying by name alone would
  collide across grids. Derive a stable, filesystem-safe per-account path (e.g.
  under a `settings/<grid>/<avatar>/` directory), matching the reference's
  `gDirUtilp->getLindenUserDir()` (which is per-account, per-grid).
- Decide which existing settings belong in the account scope vs global. The
  reference splits these as `gSavedSettings` (global) vs
  `gSavedPerAccountSettings` (account); most rendering/UI prefs are global,
  while a few (e.g. some UI/chat/privacy toggles) are per-account. A settings
  UI/editor would then need to target a scope when writing.

Reference (Firestorm, read-only): `indra/newview/llappviewer.cpp`
(`loadSettingsFromDirectory`, the `gSavedPerAccountSettings` group),
`indra/llvfs/lldir.cpp` (`getLindenUserDir` — the per-grid, per-account path),
and `LLControlGroup::loadFromFile` / `saveToFile`.

Note: the on-disk **format** is already TOML for both scopes
([[viewer-settings-toml-format]]); this task is only the viewer-side wiring and
the account-file path derivation.
