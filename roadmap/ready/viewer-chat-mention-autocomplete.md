---
id: viewer-chat-mention-autocomplete
title: Chat @-mention picker / name autocomplete
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-chat-input-bar]
refs: [viewer-emoji-colon-autocomplete, viewer-avatar-radar]
---

Context: [context/viewer.md](../context/viewer.md).

Name autocompletion in the chat inputs: typing `@` (and, optionally, a plain
name prefix) pops a completion list of candidate residents — nearby avatars
(the coarse/radar set), current IM / group-session participants, and friends —
and inserts the chosen name. The interaction pattern (inline trigger char,
anchored popup, arrow/Enter selection) already exists once in
[[viewer-emoji-colon-autocomplete]]; reuse that machinery rather than growing
a second completer.

Scope: the candidate providers (per input context: nearby chat completes from
nearby avatars, an IM tab from its participants), display-name vs username
insertion, and the mention being highlighted in the sent line for the
mentioned user (ties into keyword alerts — [[viewer-chat-keyword-alerts]]).

Reference (Firestorm, read-only): `floater_chat_mention_picker.xml`,
`llchatmentionhelper`.

Builds on: the chat input widget (`chat_input.rs`) and the emoji completer.
