---
id: viewer-preferences-colors-skins-tab
title: Preferences — colors & skins tab
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-preferences-floater, viewer-ui-color-picker]
refs: [viewer-name-tags-preferences]
---

Context: [context/viewer.md](../context/viewer.md).

The **colors & skins** tab: pick the UI **skin and theme** (our CSS skin
system already ships `azure` / `graphite` × light/dark with hot reload —
this surfaces the choice as a setting instead of a CLI flag), and edit the
user-tunable **colour tokens**: chat colours (self / others / objects /
IM), name-tag colours, friend highlight, keyword-alert colour — each a
skin-token override stored per account and applied through the existing
token cascade, edited via [[viewer-ui-color-picker]] swatches with a
reset-to-skin-default per row.

Reference (Firestorm, read-only): `panel_preferences_colors.xml`,
`panel_preferences_skins.xml`, `floater_settings_color.xml`.

Deps: [[viewer-preferences-floater]], [[viewer-ui-color-picker]].
