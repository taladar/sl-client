---
id: viewer-inventory-materials-not-shown
title: Material inventory items are not shown in the inventory window
topic: viewer
status: bugs
origin: user report (2026-07-24) while testing the PBR material editor
refs: [viewer-face-materials-pbr, viewer-pbr-material-editor]
---

Context: [context/viewer.md](../context/viewer.md).

GLTF **material** inventory items (`InventoryType::Material` /
`AssetType::Material`) do not appear in the inventory window at all — neither in
the tree nor (presumably) the gallery. Surfaced while testing the PBR material
editor: a material saved to inventory (the editor's Save) has nowhere visible to
land, and existing materials cannot be picked/assigned.

**Likely not the display layer.** The viewer already maps the Material type end
to end for *display*: `inventory.rs` gives it an icon (🎨,
`item_type_glyph` `InventoryType::Material`), and `inventory_filters.rs` maps it
to a filter bit (`InventoryFilter::Material`, `inventory-filter-materials`). So
a Material item that *reaches* the model should render. Suspect instead that
Material items are **dropped or misclassified during the inventory fetch /
decode** — the AIS (`InventoryAPIv3`) descendants parse or the UDP
`InventoryDescendents` decode in `sl-proto` — so they never enter
`InventoryModel`. Verify by logging the raw item types the fetch yields for a
folder known to contain a material, and check the `InventoryType` /
`AssetType` decode tables (`sl-proto/src/types/asset.rs`) against what AIS
returns for a material item.

Also check the **Materials system folder** (`FolderType::Material`) is decoded
and rooted — the Save path targets it (`folder_by_type(FolderType::Material)`),
so if the folder itself is missing/miscategorised the saved item is invisible
even if the item decode is correct.

**Folds in the render-material picker fix.** The Texture-tab PBR render-material
swatch ([[viewer-face-materials-pbr]]) currently opens the generic
[[viewer-ui-texture-picker]] — its dialog says "Pick Texture" and it browses
*textures*, because there is no material picker. That picker must instead
target **materials** (title "Pick Material", filtered to
`InventoryType::Material`), but that is untestable and pointless while materials
do not appear in inventory at all — so it belongs here, once material items are
visible: give the picker (or a per-open title/filter on
[[viewer-ui-texture-picker]]) a material mode, and point the render-material
swatch at it.

Reference (Firestorm, read-only): `LLInventoryType` /
`LLAssetType::AT_MATERIAL`, `llinventorymodel` AIS descendants handling,
`LLTextureCtrl` material-picker mode / `LLFloaterTexturePicker`.
