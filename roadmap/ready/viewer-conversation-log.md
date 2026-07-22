---
id: viewer-conversation-log
title: Conversation log — browse past conversations
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
refs: [chat-b9, viewer-social-im-conversations]
---

Context: [context/viewer.md](../context/viewer.md).

A floater listing **past conversations** — every partner / group / conference
the account has chat-log files for — with, per row, the display name, the last
message time, and a transcript **preview pane** that opens the stored log
read-only. The on-disk source already exists: the per-account chat-log files
([[chat-b9]], `chat_log.rs`, keyed by grid + avatar via the account dirs), so
this task is purely the read/browse UI over them plus jump-off actions (open a
live IM with that partner — [[viewer-social-im-conversations]] — or open the
profile).

Scope: enumerate the log directory into a sortable, filterable virtualized
list ([[viewer-ui-virtualized-list]]), lazy-load transcripts, search within a
transcript, and delete a transcript (with confirmation; deleting history is
destructive).

Reference (Firestorm, read-only): `llfloaterconversationlog`,
`llfloaterconversationpreview`, `llconversationlog`.

Builds on: the chat-log files ([[chat-b9]]) and the per-avatar account dirs.
