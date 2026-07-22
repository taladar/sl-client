---
id: viewer-default-creation-permissions
title: Default creation permissions
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-settings-store]
refs: [viewer-image-upload, viewer-prim-creation]
---

Context: [context/viewer.md](../context/viewer.md).

The default next-owner / group / everyone permissions applied to **newly
created** content — uploads ([[viewer-image-upload]]), created prims
([[viewer-prim-creation]]), new scripts / notecards / wearables — as a small
per-category settings floater (the reference's "Default Creation
Permissions": one row per asset category × copy / modify / transfer + share
toggles). SL applies these via the `AgentPreferences` capability
(`DefaultObjectPermissions`) plus client-side application at
create/upload time; `api-g14` already pairs the preferences caps.

Reference (Firestorm, read-only): `floater_perms_default.xml`,
`llfloaterperms`.

Builds on: the settings store and `api-g14` (AgentPreferences caps).
