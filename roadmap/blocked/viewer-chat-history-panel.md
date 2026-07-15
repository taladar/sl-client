---
id: viewer-chat-history-panel
title: Chat history panel (scrollable / resizable)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-chat-input-bar, viewer-ui-floater-basic]
---

Context: [context/viewer.md](../context/viewer.md).

Promote the fixed bottom-left `chat.rs` overlay into a proper scrollable,
resizable nearby-chat **history panel** hosted in a floater
([[viewer-ui-floater-basic]]): scrollback, resize, and a stable place for the
input bar ([[viewer-chat-input-bar]]) to dock. Chat lines feed from the existing
`ChatReceived` stream and the persisted chat log.

This is the surface [[viewer-i18n-chat-translation]] renders original +
translated text into, and where URL/SLURL linkification would decorate lines
(the separate `viewer-url-linkification` idea).

Reference (Firestorm, read-only): `fsfloaternearbychat`,
`llfloaterimnearbychat`, `llchathistory`.

Builds on: `chat.rs` overlay + the chat-log history paging in `chat_log.rs`.
