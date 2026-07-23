---
id: viewer-avatar-render-settings-manager
title: Per-avatar render-settings manager
topic: viewer
status: blocked
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-avatar-complexity-limit]
refs: [viewer-derender-blacklist]
---

Context: [context/viewer.md](../context/viewer.md).

World ▸ Avatar Render Settings: a management floater over *persistent*
per-avatar render overrides — Render Fully (exempt from complexity
limits), Do Not Render (permanent jelly), or default — surviving
relog, with add/remove/edit from the floater or the avatar context
menu.

Scope:

- A persisted per-account map avatar → override
  (fully/never/default), applied by the avatar renderer above the
  automatic complexity rules ([[viewer-avatar-complexity-limit]]).
- The management floater listing overridden avatars with mode edit and
  removal; entries addable from the avatar context menu.
- Distinct from the transient session derender
  ([[viewer-derender-blacklist]]).

Reference (Firestorm, read-only): `Floater.Toggle
fs_avatar_render_settings` (`menu_viewer.xml` World section),
`fsavatarrenderpersistence` + `llfloateravatarrendersettings`.

Builds on: the complexity-limit render pipeline (its
jelly/never-render machinery is what the overrides drive).
