---
id: viewer-derender-blacklist
title: Derender + asset blacklist
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-object-context-menu]
refs: [viewer-block-list, viewer-avatar-complexity-limit]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's derender: remove an object (or avatar) from *your* view —
temporarily (until region re-entry) or permanently via a persisted **asset
blacklist** — the everyday tool against visual griefing and against the one
laggy object a parcel owner will not remove. Client-side only, distinct from
the server mute list ([[viewer-block-list]]).

Scope: "Derender" / "Derender + blacklist" on the object and avatar context
menus, suppression at the scene-mirror level (object add/update for a
blacklisted id is dropped before meshing — cheap), the blacklist floater
(entries with name / region / date, remove / re-render), per-account
persistence, and blacklist of specific asset ids (sounds, textures) the
sound explorer and friends can feed later.

Reference (Firestorm, read-only): `fsassetblacklist`,
`floater_fs_asset_blacklist.xml`, the FS derender menu handlers.

Builds on: the object context menu and the scene mirror (`objects.rs`).
