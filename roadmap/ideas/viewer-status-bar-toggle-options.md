---
id: viewer-status-bar-toggle-options
title: Status/menu-bar display toggles
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-ui-status-bar, viewer-statistics-floater,
       viewer-i18n-number-datetime-formats]
---

Context: [context/viewer.md](../context/viewer.md).

Display toggles for the top bar. Our status bar (done
[[viewer-ui-status-bar]]) shows region/parcel, balance, time, FPS and
permission icons, but only a coordinates toggle exists in
`status_bar.rs`. The reference lets the user choose: show location in
the top bar (`ShowMenuBarLocation`), show the simulator version
(`FSStatusbarShowSimulatorVersion`), show the currency balance
(`FSShowCurrencyBalanceInStatusbar`), a traffic/net-stats indicator
(`ShowNetStats`) with its legacy mean-per-second display
(`FSStatbarLegacyMeanPerSec`) and click-opens-statistics behaviour
(pairs [[viewer-statistics-floater]]), menu-button popup on rollover
(`FSStatusBarMenuButtonPopupOnRollover`), and the clock format combo —
12/24 h, seconds, timezone (`FSStatusBarTimeFormat`). Our clock follows
the locale via ICU (done [[viewer-i18n-number-datetime-formats]]), so
the format combo likely reduces to a smaller set (e.g. SLT vs local
timezone, seconds on/off) rather than a free-form pattern.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_UI.xml`,
`indra/newview/llstatusbar.cpp`.
