---
id: viewer-chat-channel-and-commands
title: Chat channels, whisper/shout & /me
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-text-input-emoji]
---

Context: [context/viewer.md](../context/viewer.md).

Extend the chat input bar with channel and volume selection: the `/N …`
**channel prefix** (route a line to channel N), **say / whisper / shout** volume
selection, and the `/me …` **emote**. Map each to the right `Command::Chat`
channel / chat-type; the wire side already carries channel and chat-type.

Reference (Firestorm, read-only): `llchatbar` channel parsing, `LLChat`
chat-type handling.

## Done (2026-07-20)

Realized (ahead of its `viewer-chat-input-bar` blocker) as the reusable
**local-chat-input widget** — `sl-client-bevy-viewer/src/local_chat_input.rs` —
which wraps the chat-input widget ([[viewer-ui-text-input-emoji]]) with the
local-chat behaviours and, still session-free, emits a **structured** output a
live consumer maps to `Command::Chat`.

- **Whisper / Say / Shout select box** beside the emoji button (a small dropdown
  reflecting the current volume).
- **`/N …` channel routing** and the parse (`classify_line`, unit-tested):
  `/N rest` → channel N as Normal type (whisper/shout are channel-0 only); a
  bare `/` or an **unregistered** `/word` is said verbatim (which is how `/me …`
  reaches the sim as an emote).
- **Shift/Ctrl+Enter → whisper/shout** (`resolve_volume`), matching Firestorm's
  `FSUseShiftWhisper` / `FSUseCtrlShout`; both/neither leave the select box's
  choice.
- A general **`/command` registry** (`SlashCommands`, requested during the
  work): a `/word` whose non-numeric `word` is registered becomes a
  `SlashCommandInvoked` the registrant handles, not chat — so other parts of the
  viewer claim their own slash commands.

Outputs are `LocalChatSubmit` / `SlashCommandInvoked`; the live nearby-chat bar
([[viewer-chat-input-bar]]) and conversations floater
([[viewer-social-im-conversations]]) that consume them are follow-ups. Swept
live as the `local-chat-input` specimen; the parse, modifier resolution and
volume mapping are unit-tested.
