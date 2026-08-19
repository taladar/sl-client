---
id: viewer-window-attention-flash
title: Window urgency flash + in-UI unread-flash cues
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-window-title-unread-count, viewer-os-portals-linux,
       viewer-ui-bottom-toolbar]
---

Context: [context/viewer.md](../context/viewer.md).

Attention cues when the user is looking elsewhere. Window-manager
level: flash/bounce the app's attention marker while the window is
unfocused when a new IM arrives (`FSFlashOnMessage`), an object IM
(`FSFlashOnObjectIM`), or a script dialog (`FSFlashOnScriptDialog`) —
on Linux/Wayland this is the urgency hint, reachable via winit's
`request_user_attention`. In-UI level: flash the chat/IM toolbar
button and conversation tabs on unread messages
(`FSNotifyIMFlash`, `FSNotifyNearbyChatFlash`), flash on friend
online/offline status changes (`FSIMChatFlashOnFriendStatusChange`),
scrollback unread notices (`FSNotifyUnreadChatMessages` /
`FSNotifyIMMessages`), with flash cadence knobs (`FlashCount`,
`FlashPeriod`).

Complements the title-bar unread count
([[viewer-window-title-unread-count]]) and desktop notifications via
the portal ([[viewer-os-portals-linux]]); the toolbar-button flash
hangs off the shipped bottom toolbar ([[viewer-ui-bottom-toolbar]]).
Nothing in our tree raises urgency or flashes any UI element today.

Reference (Firestorm, read-only): `indra/newview/llviewerwindow.cpp`
(window flashing), `indra/newview/fsfloaterim.cpp`,
`indra/newview/llchiclet.cpp`,
`indra/newview/skins/default/xui/en/panel_preferences_chat.xml`,
`panel_preferences_alerts.xml`, `panel_preferences_UI.xml`.
