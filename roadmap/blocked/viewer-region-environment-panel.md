---
id: viewer-region-environment-panel
title: Region / parcel environment settings panel
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-region-options-general, viewer-environment-my-environments]
refs: [viewer-parcel-options-general]
---

Context: [context/viewer.md](../context/viewer.md).

Publishing environments to land: the Region/Estate floater's
**Environment** tab and the parcel-level equivalent in About Land — choose
a day cycle (from the picker of [[viewer-environment-my-environments]]) or
the legacy defaults, set day length / offset, manage the altitude sky
tracks, apply / reset — written through the `ExtEnvironment` capability
(get/put per region or parcel; the parcel variant carries the parcel id).
The ingest side of `ExtEnvironment` exists (P22 reads region environments);
this task adds the **write** pairing in `sl-proto` plus the two panels.

Reference (Firestorm, read-only): `llpanelenvironment` /
`panel_region_environment.xml`, `llenvironment` (`ExtEnvironment` PUT).

Deps: [[viewer-region-options-general]] (the region floater the tab lives
in), [[viewer-environment-my-environments]] (the settings picker).
