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

Audit 2026-10-15: the on-disk side has grown since — the chat log now lives
in `sl-client-common/src/chat_log.rs` and keeps the reference's
`conversation.log` **index** (one line per conversation with its last-activity
stamp, purged after `ConversationLogRetentionDays`; the line codec is
`sl_proto::conversation_log_line` / `conversation_log_file`), toggled by the
`KeepConversationLog` preference (`sl-viewer-preferences/src/
preferences_chat.rs`). The floater should list that index rather than walk
the directory. No browse UI exists yet; that is what remains.
