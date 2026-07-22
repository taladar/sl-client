---
id: viewer-environment-my-environments
title: My Environments library
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-environment-fixed-editor]
refs: [viewer-environment-personal-lighting, viewer-region-environment-panel]
---

Context: [context/viewer.md](../context/viewer.md).

The "My Environments" library floater: every settings asset in inventory
(sky / water / day cycle, filterable), with **apply to self** (the local
override layer of [[viewer-environment-personal-lighting]]), edit (opens
the matching editor), rename / delete, and the settings-asset **picker**
widget other panels summon (the day-cycle track editor, the region/parcel
environment panel [[viewer-region-environment-panel]]). The Linden library
folder's stock environments appear alongside the user's own.

Reference (Firestorm, read-only): `llfloatermyenvironment`,
`floater_my_environments.xml`, `floater_settings_picker.xml`.

Deps: [[viewer-environment-fixed-editor]] (asset model + editors).
