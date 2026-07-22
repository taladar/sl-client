---
id: viewer-god-tools
title: God tools floater
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-region-options-general]
---

Context: [context/viewer.md](../context/viewer.md).

The admin/god floater for accounts with god level (OpenSim grid-god
accounts make this locally testable): the **Grid** tab (kick/freeze user,
flush map visibility), **Region** tab (the region flag toggles and the
"bake terrain" / region file actions beyond the estate-manager set),
**Objects** tab (owner-wide delete, get owner), and **Request** tab (the
generic `GodlikeMessage`). The wire is done — the god batches
(`missing-out-batch-9/-10`) cover `GrantGodlikeExpiry`, god kicks, forced
land actions and the godlike messages; QA/admin-status visibility gates the
menu entry.

Reference (Firestorm, read-only): `llfloatergodtools`,
`floater_god_tools.xml`.

Builds on: the god/admin protocol batches; god-level state from login /
`GrantGodlikePowers`.
