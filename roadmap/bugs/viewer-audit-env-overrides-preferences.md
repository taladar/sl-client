---
id: viewer-audit-env-overrides-preferences
title: Environment variables silently override live graphics preferences
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

Several `SL_VIEWER_*` variables override a **registered, GUI-editable** setting
with no indication in the UI, so the checkbox or slider moves and does nothing:

- `sl-viewer-world-scene/src/tonemap.rs:205-219` — `SL_VIEWER_TONEMAP`,
  `_MIX`, `SL_VIEWER_EXPOSURE` beat the stored `RenderTonemapType` /
  `RenderTonemapMix` / `RenderExposure` (bound at
  `sl-viewer-preferences/src/preferences_graphics.rs:536`, `:556`, `:564`), and
  `refresh_tonemap_settings` re-reads them **per camera per frame**;
- `sl-viewer-preferences/src/preferences_graphics.rs:606`, `:664` —
  `SL_VIEWER_SUN_SHADOWS`, `SL_VIEWER_SHADOW_CASCADES`;
- `sl-viewer-world-scene/src/{glow,exposure,particles}.rs` —
  `SL_VIEWER_DISABLE_GLOW`, `SL_VIEWER_GLOW_STRENGTH`,
  `SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE`, `SL_VIEWER_DISABLE_HUD_PARTICLES`.

Two authorities for one user-facing value, and the env one wins silently.
Related: `SL_VIEWER_SKIN` / `SL_VIEWER_THEME`
(`sl-viewer-ui-core/src/skin.rs:99`, `:102`) are a **third** authority alongside
the Colors & Skins tab and the `--skin` / `--theme` flags, resolved in
`SkinSelection::resolve`; and `SL_VIEWER_UI_LOCALE` / `_DIRECTION`
(`i18n.rs:99`, `:337`) shadow a registered locale setting.

The project rule is: GUI options live in preferences, CLI is for
non-GUI/startup concerns, env vars are for source-level debugging only. Scope:
either make these debug-only knobs that log loudly when they win, or drop them
in favour of the preference. Two more that drive user-facing behaviour rather
than debugging: `SL_VIEWER_GPU_AVATARS` / `_SPIKE` (`lib.rs:1674`, `:1683`)
select the rendering path, and `SL_VIEWER_CROWD` adds a visible toolbar button —
`CrowdDebugButtonPlugin` is added unconditionally to the live viewer at
`lib.rs:1186`.

For the whole picture: 93 distinct `SL_VIEWER_*` variables exist across 17
crates, of which about 60 are legitimate `SL_VIEWER_LOG_*` / `_DISABLE_*` /
budget knobs. Roughly 89 have no CLI or preference counterpart at all, so they
are undiscoverable and unlisted by `--help`.
