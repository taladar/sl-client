---
id: viewer-chat-channel-and-commands
title: Chat channels, whisper/shout & /me
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-chat-input-bar]
---

Context: [context/viewer.md](../context/viewer.md).

Extend the chat input bar with channel and volume selection: the `/N …`
**channel prefix** (route a line to channel N), **say / whisper / shout** volume
selection, and the `/me …` **emote**. Map each to the right `Command::Chat`
channel / chat-type; the wire side already carries channel and chat-type.

Reference (Firestorm, read-only): `llchatbar` channel parsing, `LLChat`
chat-type handling.
