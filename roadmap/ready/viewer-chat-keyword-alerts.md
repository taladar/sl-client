---
id: viewer-chat-keyword-alerts
title: Chat keyword alerts
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-chat-history-panel]
refs: [viewer-chat-mention-autocomplete, viewer-ui-sound-effects]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's keyword-alert feature: a user-configured list of words / phrases
(their own name being the classic entry) that, when they appear in incoming
chat or IM, highlight the matching line in an alert colour, and optionally
play a sound and flash the conversation tab. Being @-mentioned
([[viewer-chat-mention-autocomplete]]) is a mention-shaped instance of the
same alert path.

Scope: the keyword list + match options (case sensitivity, whole-word, local
chat vs IM vs group scopes), the line-highlight in the chat displays, the
alert colour token in the skin system, the optional sound hook (lands with
[[viewer-ui-sound-effects]]; until then the visual alert stands alone), and
settings persistence.

Reference (Firestorm, read-only): the FS chat preferences keyword section
(`panel_preferences_chat`, `FSKeyword*` settings), `fskeywords`.

Builds on: the chat history displays and the settings store.
