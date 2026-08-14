---
id: viewer-preferences-colors-skins-tab
title: Preferences — colors & skins tab
topic: viewer
status: done
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

Shipped: skin defaults live in each skin's CSS (`--chat-*`, `--keyword-alert`,
`--name-tag-*` tokens) and flow into the settings store as **dynamic declared
defaults** (`skin_colors.rs` reads the styled root's resolved vars via
`SettingsStore::set_default`), so per-account `Color3` overrides sit above the
skin and per-row Reset falls back to it live. The skin/theme choice persists as
global `UiSkin` / `UiSkinTheme` (CLI `--skin`/`--theme` still overrides per
run); switching re-dresses live — no restart. Beyond the roadmap list, the
system-chat colour and the four name-tag distance-band colours got rows too
(real consumers existed), and the keyword-alert row shipped **ahead of its
consumer** ([[viewer-chat-keyword-alerts]] wires the highlight; a user-approved
exception to the no-dead-code rule).
