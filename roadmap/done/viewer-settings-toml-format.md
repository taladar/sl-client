---
id: viewer-settings-toml-format
title: TOML settings file with comments and nested sections
topic: viewer
status: done
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

## Done

New `sl-settings::toml_format` module replaces the `serde_json` persistence;
`serde_json` is dropped for `toml_edit` (comment-aware TOML). Both `save_scope`
and `load_scope` (thus both the `Global` and `Account` scopes, split unchanged)
now round-trip through it.

Design choices worth carrying forward:

- **No type tag on disk.** The old JSON was adjacently-tagged
  (`{"type":"f32","value":1.5}`); the TOML is a bare `name = value`. The type is
  recovered from the setting's *declaration* on load (like the reference's
  `settings.xml`), so the file stays hand-editable. The declaration is what
  disambiguates the shape-collisions (`Color3` vs `Vec3` vs `Vec3d` are all
  3-float arrays; `Rect` vs `Color4` are both 4-element). A value that no longer
  fits its declared type is dropped (the old "type changed" case); an
  *undeclared* setting (forward-compat) is kept by inferring a `SettingValue`
  from the TOML shape — lossy in the arbitrary way (a 3-float array → `Vec3`)
  but re-saves the same on-disk value.
- **Grouping metadata** is a `section: Vec<String>` on `SettingDecl`, set via
  the new `register_in(&["a","b"], …)` (plain `register` = document root).
  Renders as nested `[a.b]` tables; pure-parent sections are `set_implicit` so
  only the deepest dotted header prints. The viewer's spacenav settings register
  under `[spacenav.flycam]` — mode-specific on purpose (the reference has
  parallel `Avatar*`/`Build*` joystick families), leaving room for
  `[spacenav.avatar]` / `[spacenav.build]`.
- **f32 formatting:** values are written from the shortest `f32` repr (parsed
  into a `toml_edit::Value`), not widened to `f64` first, so `0.1` stays `0.1`
  instead of `0.10000000149…`. Read back by parsing the source repr as `f32`
  (single rounding), so an `f32` round-trips exactly.
- The viewer file is now `viewer-settings.toml` (was `.json`); an old `.json` is
  silently ignored (treated as first run) — no migration, acceptable for the dev
  viewer's spacenav-only settings.

Follow-up filed: [[viewer-settings-account-scope-persist]] — the viewer still
wires only the `Global` scope; the per-account file (keyed by **grid + avatar
name**) is unwired.

Verified: `cargo test -p sl-settings` (14 tests incl. all-value-types
round-trip, sections+comments, forward-compat unknowns), clippy-clean on both
crates, viewer builds. No live-grid run needed — pure client-side serialisation.
