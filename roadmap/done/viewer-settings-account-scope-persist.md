---
id: viewer-settings-account-scope-persist
title: Load and save both global and per-account settings in the viewer
topic: viewer
status: done
origin: noticed wiring viewer-settings-toml-format (2026-07)
blocked_by: [viewer-ui-settings-store]
refs: [viewer-settings-toml-format, viewer-chat-log-grid-key]
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

## Done

Scope grew (at the user's direction) beyond just settings, into the whole
per-avatar on-disk layout, using the `directories` crate for the base paths.

**New crate `sl-account-dirs`** owns the per-avatar directory policy:
`<accounts_base>/<grid>/<name>/` keyed by grid + readable avatar name, with a
`<grid>/.by-uuid/<uuid> → <name>` reverse-index symlink. One idempotent
`reconcile_account_dir(base, grid, name, uuid)` creates the directory or, when
the reverse index shows the UUID under a *different* name (a paid Linden name
change), **renames it in place** so the data follows the rename — discovered and
handled at the synchronous login point, no re-login (so the Aditi cooldown is
never touched). Grid is *always* in the path because Aditi is cloned from Agni
and can share a UUID. `grid_dir_name` = login-URI host (`:port` kept).

**Why name-keyed dirs, UUID-indexed:** the name is known before login and the
UUID only from the login response — but the login response arrives before the
simulator/CAPS connection, so the UUID is known before any per-avatar file is
touched. The reconcile runs inline at that point.

**Wiring:**

- New viewer `paths.rs` resolves config / cache / data roots via `directories`
  (`ProjectDirs::from("net","taladar","sl-client-bevy-viewer")`). The five asset
  caches (texture/mesh/material/animation/bake) moved onto the cache root — same
  paths as before, so no cache invalidation. Global settings moved from the CWD
  to `<config>/viewer-settings.toml`.
- `sl-client-bevy` `SlClientPlugin` gained an optional `AccountDirsConfig`
  (accounts base + grid + name). At login (`advance_login`, once the UUID is
  known, before the shells are built) it reconciles the directory and points the
  chat-log / inventory-cache dirs at `<dir>/chat` / `<dir>/inventorycache`.
- The viewer enables **chat logging** (all four `LoggedChatType`s) and the
  **inventory disk cache**, both now landing in the per-avatar directory.
- Account settings: `ViewerSettings` loads the `Global` scope at startup and the
  `Account` scope (`<dir>/settings.toml`) via `load_account_settings` once the
  agent UUID appears in `SlIdentity`; both scopes save on logout. The account
  reconcile is a second idempotent call, so it agrees with the plugin's.

No setting is *account-only* yet — the mechanism is wired and specific
per-account settings can now join. The two-avatar layering direction (which
settings are account vs global) is deferred to whenever a settings UI lands.

Follow-up [[viewer-chat-log-grid-key]]: the two REPL binaries
(`sl-repl-tokio` / `sl-repl-bevy`) still pass a bare `--chat-log-dir`, so their
transcripts are name-keyed with no grid — to be fixed with the same
`sl-account-dirs` resolver.

Verified: `cargo test -p sl-account-dirs` (reconcile + rename-discovery +
cross-grid), clippy-clean, all crates build. Live login-path exercise is left to
a real grid run (creates the directory tree).
