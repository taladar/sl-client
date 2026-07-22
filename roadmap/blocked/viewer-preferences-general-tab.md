---
id: viewer-preferences-general-tab
title: Preferences — general tab
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-preferences-floater]
refs: [viewer-i18n-locale-selection, viewer-do-not-disturb-away]
---

Context: [context/viewer.md](../context/viewer.md).

The **general** tab of the preferences floater
([[viewer-preferences-floater]]): interface language
([[viewer-i18n-locale-selection]] provides the mechanism), maturity rating
preference (the `AgentPreferences`/wire write behind General→maturity),
start location default (last / home), UI scale, name-tag basics (the
handful of headline toggles; the full set lives in
[[viewer-name-tags-preferences]]), away timeout
([[viewer-do-not-disturb-away]] consumes it), and the busy-response text
fields.

Reference (Firestorm, read-only): `panel_preferences_general.xml`.

Deps: [[viewer-preferences-floater]].
