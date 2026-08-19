---
id: viewer-chat-transcript-style-options
title: Chat / IM transcript display and style options
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-chat-history-panel, viewer-social-im-conversations,
  viewer-chat-timestamps, viewer-conversation-log,
  viewer-conversations-session-restore,
  viewer-chat-input-behavior-options]
---

Context: [context/viewer.md](../context/viewer.md).

The reference keeps a large family of transcript-presentation options on
the chat / IM header menus and the chat preferences pages; our
conversations UI ([[viewer-chat-history-panel]] and
[[viewer-social-im-conversations]], both done) renders one fixed
presentation — sender plus text with the five ChatColor* classes — and
exposes none of them. Chat font size already exists
(`preferences_chat.rs` ChatFontSize); timestamp toggles belong to
[[viewer-chat-timestamps]].

View-mode options (menu_im_session_showmodes.xml, menu_nearby_chat.xml,
menu_participant_view.xml): **compact vs expanded** transcript view, a
toggle to **show names in 1:1 conversations** (`IMShowNamesForP2PConv`),
header **icons / names / icons+names**, profile icons in chat headers,
chat mini-icons (`ShowChatMiniIcons`), Firestorm's **V1-style
plain-text headers** (`PlainTextChatHistory`), the chevron typing
indicator, classic V1 console modes (`FSConsoleClassicDrawMode`,
`ChatFullWidth`, `FSUseNearbyChatConsole`), and conversation-list
sorting by type / name / recent activity. Chat-bar element toggles are
split out to [[viewer-chat-input-behavior-options]].

Text decorations, each settings-gated in the reference: group-name
prefix on group-chat lines with a max length (`FSShowGroupNameLength`),
localized **"You"** for own lines (`FSChatHistoryShowYou`), bold shouts
and italic whispers (`FSEmphasizeShoutWhisper`), emote italics
(`EmotesUseItalic`), group-moderator highlighting
(`FSHighlightGroupMods` + `FSModNameStyle` / `FSModTextStyle`),
distinct IM/group colouring in the console (`FSColorIMsDistinctly`),
username coloured separately from the display name (`FSColorUsername`),
diminished colour for chat from beyond hearing range
(`FSBeyondNearbyChatColorDiminishFactor`), square-bracketed system
messages (`FSIMSystemMessageBrackets`), IM-history fade factor
(`FSIMChatHistoryFade`), the muted-text display toggle
(`FSShowMutedChatHistory`), and the "(no name)" object anti-spoof mark
(`FSMarkObjects`).

Session/routing options: show IMs / group chat in the nearby transcript
(`FSShowIMInChatHistory` / `FSLogIMInChatHistory`), IM and group popups
to the console (`FSLogImToChatConsole`, `FSLogGroupImToChatConsole`,
`EnableGroupChatPopups` / `EnableIMChatPopups`), group notices into the
group transcript (`FSGroupNoticesToIMLog`), group-name tab length,
show-end-of-last-conversation (`LogShowHistory`, transcript source per
[[viewer-conversation-log]]), and open-Conversations-on-offline-
messages (`FSOpenIMContainerOnOfflineMessage`); restoring open
conversations on relog (`FSRestoreOpenIMs`) is its own task,
[[viewer-conversations-session-restore]]. Implementation is one
settings section plus appliers in the existing transcript composer in
`sl-client-bevy-viewer/src/conversations.rs` / `chat.rs` — each toggle a
setting and a branch in the transcript builder.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_im_session_showmodes.xml`,
`menu_nearby_chat.xml`, `menu_participant_view.xml`,
`panel_preferences_chat.xml`; `indra/newview/llchathistory.cpp`,
`indra/newview/fsfloaternearbychat.cpp`, `indra/newview/fsfloaterim.cpp`.
