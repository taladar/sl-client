---
id: viewer-opensim-region-settings-panel
title: OpenSim OpenRegionSettings tab in the Region / Estate floater
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-region-options-general, viewer-opensim-region-extras-limits,
  test-open-region-info]
---

Context: [context/viewer.md](../context/viewer.md).

On OpenSim grids Firestorm shows an extra Region Settings tab
(LLPanelRegionOpenSettingsInfo,
`panel_region_open_region_settings.xml`) driven by the
OpenRegionSettings/OpenRegionInfo extras: draw-distance force, prim
scale min/max, hollow/hole-size limits, link counts, and
minimap/physical-prim/water toggles, plus windlight-per-parcel.

We already decode the OpenRegionInfo limits bag ([[test-open-region-info]]
done) but the About Region floater
(`sl-client-bevy-viewer/src/about_region.rs`) has no such tab.
Implementing this means an OpenSim-gated tab rendering the decoded
limits and, for estate owners, the write path back. OpenSim-only, so
low priority (SL is the primary target); the broader OpenSim extras
story lives in [[viewer-opensim-region-extras-limits]].

Reference (Firestorm, read-only):
`indra/newview/llfloaterregioninfo.cpp` (LLPanelRegionOpenSettingsInfo),
`indra/newview/skins/default/xui/en/panel_region_open_region_settings.xml`.
