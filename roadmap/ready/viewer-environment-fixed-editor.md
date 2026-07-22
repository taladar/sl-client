---
id: viewer-environment-fixed-editor
title: Environment editors — sky & water settings assets
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-environment-personal-lighting, viewer-environment-day-cycle-editor, viewer-environment-my-environments]
---

Context: [context/viewer.md](../context/viewer.md).

The fixed-environment editors: create and edit **sky** and **water**
settings as EEP **inventory assets** (`AssetType::Settings`, flags sky /
water / daycycle). Tabbed panels mirroring the reference — sky: atmosphere
& haze, clouds (texture, coverage, scroll), sun & moon (textures, position,
brightness) and the density sections; water: fog, fresnel, normal map,
wave directions — every field the `SkySettings` / water types already
ingest, now editable with live preview through the local-override layer of
[[viewer-environment-personal-lighting]].

Save path: settings assets serialize as LLSD and upload via the standard
asset/inventory create-update flow (`sl-llsd` + `upload.rs`); load path:
apply from inventory. The library ships Linden defaults to start from.

The day-cycle editor and the environments library build on this
([[viewer-environment-day-cycle-editor]],
[[viewer-environment-my-environments]]).

Reference (Firestorm, read-only): `llfloaterfixedenvironment`,
`llfloatereditextdaycycle` (shared panels), `panel_settings_sky_*.xml`,
`panel_settings_water.xml`, `llsettingsvo` (asset serialisation).

Builds on: EEP ingest types, `sl-llsd`, the asset upload path.
