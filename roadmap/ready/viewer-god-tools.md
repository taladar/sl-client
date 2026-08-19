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

## Parity-audit addendum (2026-08-19)

The parity audit adds the Admin menu's selection-context actions,
which the floater-tab scope does not cover (the protocol wire is done
per missing-out-batch-9/10): Admin ▸ Object — **Take Copy, Force Owner
To Me, Force Owner Permissive, Delete, Lock, Get Assets IDs**; Admin ▸
Parcel — **Force Owner To Me, Set to Linden Content, Claim Public
Land**; Admin ▸ Region — **Dump Temp Asset Data, Save Region State**
(menu_viewer.xml L5932–6057). Also add the Avatar/Develop menu
entries **Request Admin Status** (Ctrl+Alt+G) / **Leave Admin Status**
(and the admin-status-gated Show Admin Menu visibility), matching the
task's existing admin-status gating scope.

Add the god-only **land auction** floater (`llfloaterauction.cpp`,
`floater_auction.xml`), reached from About Land General's "Linden
Sale…" button: snapshot the parcel and put it up for Linden auction.
God-gated admin surface; the god-tools region tabs already covered by
this task are its natural home.
