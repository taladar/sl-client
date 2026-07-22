---
id: viewer-preferences-alerts-tab
title: Preferences — alerts / popups tab
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-preferences-floater]
refs: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The **alerts** tab: per-notification enable/disable — the two lists the
reference shows ("always show" / "never show") over every suppressible
notification the host ([[viewer-ui-notification-host]]) registers, driven
by a per-notification `show_again` flag in the settings store, plus the
headline toggles (friend online/offline notices, group notice toasts,
inventory-offer auto-accept behaviour). Requires the notification host to
expose its registry of notification ids + descriptions — add that hook
there when this lands.

Reference (Firestorm, read-only): `panel_preferences_alerts.xml`,
`llnotifications` (ignore settings).

Deps: [[viewer-preferences-floater]].
