---
id: viewer-build-creation-defaults
title: Default parameters for newly created prims
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-build-create-tool-options, viewer-default-creation-permissions,
       viewer-prim-creation]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's Build preferences (the `FSBuildPrefs_*` family, surfaced on
`panel_preferences_firestorm.xml` and applied by `fspanelprefs.cpp` /
`lltoolplacer.cpp`) let the user define what a freshly rezzed prim
looks like: default size X/Y/Z, material, default texture
(`FSDefaultObjectTexture`), color, alpha, glow, fullbright, shiny, and
the phantom / physical / temporary flags. Per-account extras
auto-populate new prims: embed a chosen inventory item into every new
prim (`FSBuildPrefs_Item` / `FSBuildPrefs_EmbedItem`) and use a custom
script template instead of the default "new script"
(`FSBuildPrefs_UseCustomScript` / `FSBuildPrefs_CustomScriptItem`).
Also in the family: pivot/axis defaults (actual-root axis, pivot X/Y/Z
plus percent mode), rez-under-land-group (`RezUnderLandGroup`), and the
build-tool decimal precision (`FSBuildToolDecimalPrecision`).

Our Create tool (done [[viewer-prim-creation]]) always rezzes the grid
default plywood cube; only the default *permission* bits are tracked
separately in [[viewer-default-creation-permissions]], and the Create
tool's own option row belongs to [[viewer-build-create-tool-options]].
Implementing this means a settings group, a preferences section, and an
apply pass after ObjectAdd — the flags/size ride the add message where
possible, while texture/color/material and the embedded item/script are
follow-up ObjectImage/ObjectMaterial/RezScript-style updates the way
Firestorm applies them post-rez.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_firestorm.xml`,
`indra/newview/fspanelprefs.cpp`, `indra/newview/lltoolplacer.cpp`,
`indra/newview/llpanelobject.cpp` (FSBuildPrefs use),
`indra/newview/app_settings/settings.xml` +
`settings_per_account.xml` (FSBuildPrefs_*).
