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

Reference (Firestorm, read-only): the language preference in
`llfloaterpreference`, `LLUI::getLanguage`.
