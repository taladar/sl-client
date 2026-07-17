---
id: viewer-settings-toml-format
title: TOML settings file with comments and nested sections
topic: viewer
status: ready
origin: split from the camera-system pass (wiring viewer-ui-settings-store into the app)
blocked_by: [viewer-ui-settings-store]
---

Context: [context/viewer.md](../context/viewer.md).

The viewer settings store (`sl_settings`, wired into the app as
`crate::settings::ViewerSettings` during the camera pass) persists to disk as
**JSON** — functional, but hard to read and hand-edit, and JSON has no comments.

Move the on-disk format to **TOML**, with:

- **A comment per setting**, from the store's declared `SettingDecl` comment
  (the reference viewer's settings.xml carries the same descriptions).
- **Nested sections** grouping related settings (e.g. `[spacenav.flycam]` with
  the per-axis scales / dead-zones under it), not a flat `name = value` list —
  so a file with as many settings as the reference stays navigable.

This is a change to the `sl-settings` crate's `save_scope` / `load_scope`
serialisation (currently `serde_json`), plus the grouping metadata each setting
needs to be placed in its section. Keep the account / global scope split and the
declared-default resolution.
