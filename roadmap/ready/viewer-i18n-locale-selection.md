---
id: viewer-i18n-locale-selection
title: Locale detection, override & runtime switch
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-i18n-localization
blocked_by: [viewer-i18n-fluent-scaffold, viewer-ui-settings-store]
---

Context: [context/viewer.md](../context/viewer.md).

Locale selection on top of [[viewer-i18n-fluent-scaffold]]: detect the OS
locale, let the user override it (persisted in [[viewer-ui-settings-store]]),
and switch the active `bevy_fluent` locale at **runtime** — re-resolving all
visible strings and flipping the layout direction (LTR/RTL) without a restart.
The content-driven auto-layout means a locale switch reflows panels to the new
string lengths automatically.

## Already done (audit 2026-10-15)

The override and the runtime switch exist. `sl-viewer-ui-core/src/i18n.rs`
registers the persisted `UiLanguage` setting (`SETTING_UI_LANGUAGE`, global
scope), and `apply_locale_setting` applies it **live** through
`set_locale_choice`, which re-negotiates the Fluent bundle and sets
`UiLocale::direction` (LTR / RTL) — no restart. The preferences General tab
(`sl-viewer-preferences/src/preferences_general.rs`, `build_general_tab`)
binds a language combo to it, and `SL_VIEWER_UI_LOCALE` seeds the start-up
locale.

## What remains

- **OS locale detection.** With no stored override ("Default") the viewer
  starts in English (or the `SL_VIEWER_UI_LOCALE` seed); nothing reads the OS
  locale (`LANG` / `LC_MESSAGES`, or a `sys-locale`-style crate on other
  platforms). Detect it and negotiate it against the shipped bundles, falling
  back to English.
- The combo's language list is hand-written in `build_general_tab`; derive it
  from the shipped bundles so a new locale
  ([[viewer-i18n-target-languages]]) appears without a code change.

Reference (Firestorm, read-only): the language preference in
`llfloaterpreference`, `LLUI::getLanguage`.
