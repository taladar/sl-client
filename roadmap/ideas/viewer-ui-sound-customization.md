---
id: viewer-ui-sound-customization
title: User-customizable UI event sounds
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-ui-sound-effects, viewer-avatar-radar,
       viewer-chat-keyword-alerts, viewer-money-economy-ui]
---

Context: [context/viewer.md](../context/viewer.md).

The reference lets the user replace every UI sound with an arbitrary
sound-asset UUID and set a per-sound play mode — new-session-only /
every message / only-when-unfocused / mute — with preview and
reset-to-default buttons per row (`panel_preferences_sound.xml`
ui_sound rows, `PlayModeUISnd*`). Our `ui_sounds.rs` (done
[[viewer-ui-sound-effects]]) has a fixed skin-supplied registry with
per-sound on/off only — no user UUIDs, no play modes, no preview.

The same pattern covers the per-event enable+UUID pairs scattered
through the FS prefs: radar enter/leave/age-alert sounds
(`UISndRadar*`, for the done [[viewer-avatar-radar]]), the
keyword-alert sound (`UISndFSKeywordSound` + `FSKeywordPlaySound`,
pairs [[viewer-chat-keyword-alerts]]), the teleport-out sound
(`PlayModeUISndTeleportOut`), friend online/offline sounds
(`PlayModeUISndFriendOnline` / `PlayModeUISndFriendOffline`), typing
sound mode (`PlayModeUISndTyping`), and the money-change sound with
its minimum-amount threshold (`UISndMoneyChangeThreshold`, pairs
[[viewer-money-economy-ui]]). Implementation: extend the sound
registry with per-event user override (UUID + play-mode enum) settings
layered over the skin defaults, plus a preferences table with preview.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_sound.xml`,
`indra/newview/llvieweraudio.cpp`,
`indra/newview/app_settings/settings.xml` (UISnd*, PlayModeUISnd*).
