---
id: viewer-chat-input-behavior-options
title: Chat-input behaviour options — send modifiers, OOC, bar toggles
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-chat-input-bar, viewer-p31-9, viewer-chat-autoreplace,
       viewer-chat-mention-autocomplete, viewer-gesture-runtime,
       viewer-chat-input-world-autostart]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer's chat bar carries a family of send-time and
layout conveniences, none of which exist in our `chat_input.rs` /
`nearby_chat_bar.rs` / `local_chat_input.rs` — the chat input sends the
typed text verbatim on Enter today. Send-time transforms: MU*-pose
(a leading ":" is treated as `/me`, `AllowMUpose`), OOC auto-close
(typing "((" auto-appends "))" — `AutoCloseOOC`, an un-prefixed but
FS-authored setting), and the modifier-send chords — Ctrl+Enter shouts
(`FSUseCtrlShout`), Shift+Enter whispers (`FSUseShiftWhisper`), and
Alt+Enter wraps the message in configurable OOC markers (`FSUseAltOOC`
with `FSOOCPrefix` / `FSOOCPostfix`). Firestorm implements the chord
handling in `llchatentry.cpp` / `fsnearbychatbarlistener.cpp`.

Chat-bar element toggles: autohide the main chat bar
(`AutohideChatBar`), show/hide the channel selector (`FSShowChatChannel`),
the chat-type (say/shout/whisper) button (`FSShowChatType`), the IM send
button (`FSShowIMSendButton`), a chat bar embedded in the Nearby Chat
window (`FSNearbyChatbar`), and focus behaviour on send —
`CloseChatOnReturn` plus its mouselook-only / nearby-only /
unfocus-history variants (`FSUnfocusChatHistoryOnReturn`, etc.).

Typing-indicator knobs round it out: [[viewer-p31-9]] built the typing
animation and sound but no toggles — play typing animation
(`PlayTypingAnim`), also-when-emoting (`FSTypeDuringEmote`), typing
sound on/off (`PlayModeUISndTyping`), and the send-typing-state privacy
switch (`FSSendTypingState`, stop broadcasting AgentUpdate typing state
to others). Gesture autocomplete in the chat bar
(`FSChatbarGestureAutoCompleteEnable`) belongs to
[[viewer-gesture-runtime]]; name/mention prediction to
[[viewer-chat-mention-autocomplete]]. Implementation is a settings
section plus appliers in the chat-input widgets.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_chat.xml`,
`indra/newview/llchatentry.cpp` (Alt/Ctrl/Shift Enter handling),
`indra/newview/fsnearbychatbar.cpp`,
`indra/newview/fsnearbychatbarlistener.cpp`,
`indra/newview/app_settings/settings.xml` (AutoCloseOOC, FSOOC*).
