---
id: viewer-notification-toast-tuning
title: Notification toast layout & lifetime tuning
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-ui-notification-host, viewer-notification-history]
---

Context: [context/viewer.md](../context/viewer.md).

Layout and lifetime knobs for the toast system. The reference exposes
per-kind toast lifetimes and fade times — notification (`ToastLifetime`
and fade), tip (`TipToastLifetime`), nearby-chat and startup toasts —
plus geometry: gap between toasts, bottom margin, nearby-toast width
and screen offsets, overflow-toast height, show-toasts-in-front of
other UI (`FSShowToastsInFront`), and where group notices appear —
bottom-right toast flow vs top-right (`ShowGroupNoticesTopRight`).

Our toast host (done [[viewer-ui-notification-host]]) exposes only the
nearby-toast lifetime today (the row in `preferences_chat.rs`); all
other durations and the layout metrics are hardcoded. The
notification-well layout options in the same FS panel belong to
[[viewer-notification-history]]. Implementation: a settings group read
by the toast host's spawn/layout systems and a small preferences
section.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_UI.xml`,
`indra/newview/llchannelmanager.cpp`,
`indra/newview/llscreenchannel.cpp`.
