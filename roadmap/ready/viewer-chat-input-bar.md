---
id: viewer-chat-input-bar
title: Chat input bar (local chat + focus)
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-text-input-widget, viewer-input-focus-contexts, viewer-ui-settings-store]
---

Context: [context/viewer.md](../context/viewer.md).

The core "chat focus" flow: a chat input bar built on
[[viewer-ui-text-input-widget]]. Enter focuses it (switching to the **Chat**
input context), Esc blurs back to World; sending on Enter emits local chat via
`Command::Chat` (the wire path `send_chat_from_viewer` already exists) and
drives the pre-wired `typing.rs::TypingState::set()` hook (P31.9 shipped the
typing animation explicitly waiting for a real input box).

Also the **settings-gated "typing in the world auto-starts local chat"**
behaviour (Firestorm's "letter keys start local chat"): when the setting (in
[[viewer-ui-settings-store]]) is on, a printable keypress while in the World
context opens the bar and forwards that character into it.

Chat receive already works (`chat.rs` overlay, `ChatReceived`). Channel /
whisper-shout / `/me` selection is [[viewer-chat-channel-and-commands]]; the
scrollable history panel is [[viewer-chat-history-panel]].

Reference (Firestorm, read-only): `fsfloaternearbychat` input, `llchatbar`,
`llnearbychatbar`.

Builds on: `protocol-1` local chat, `chat.rs`, `typing.rs`. Supersedes the MVP
"no chat input" non-goal.
