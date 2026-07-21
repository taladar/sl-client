---
id: viewer-chat-history-panel
title: Chat history panel (scrollable / resizable)
topic: viewer
status: done
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

## Done (2026-07-21)

The scrollable / resizable
**nearby-chat history panel in a floater with a docked input** was already
delivered as the **Conversations floater's Nearby tab**
([[viewer-social-im-conversations]]) — transcript scrollback, resize, and the
local-chat input docked into the pane. What this task added on top is the one
piece that was missing: **feeding the panel from the persisted chat log** so it
opens on *previous-session* history, not just the live session (the reference
viewer's `llchathistory` "load previous conversation" recall).

Nearby chat is deliberately **not** a `ChatSessionKind` — it has no participant
roster / session id / in-memory ring — so the existing
`QueryChatHistoryPage` bridge could not page it. Added a parallel nearby path:

- **Library (`sl-proto`):** new `NearbyHistoryLine { speaker, text, timestamp }`
  value type (the transcript stores a display-name *string*, not a resolvable
  key, so this is distinct from the keyed `SessionMessage`);
  `Command::QueryNearbyChatHistoryPage { already_shown, before, limit }` →
  `Event::NearbyChatHistoryPage { lines, prev }`.
- **Runtime shells (`sl-client-bevy` + `sl-client-tokio`, byte-identical):**
  `ChatLog::read_nearby_older_page(already_shown, consumed, limit)` reads the
  flat `chat.txt` transcript, `parse_log_lines` it, and pages newest-first with
  the same skip discipline as `read_older_page` — `already_shown` skips the
  newest lines the caller already shows live (they duplicate the file tail from
  this session), so recall only surfaces *older* history. Both runtimes handle
  the new command; a `query_nearby_chat_history_page` REPL command exposes it.
- **Viewer (`conversations.rs`):** a one-shot `request_nearby_recall` fires
  `QueryNearbyChatHistoryPage` once login sets the identity, with
  `already_shown` = the live nearby line count; the reply is reversed to
  oldest-first and set as the Nearby tab's `recall` lines, rendered **above**
  the live lines (`format_transcript` now takes an iterator, chaining
  `recall.iter().chain(lines.iter())`). Recall lines bypass the live
  `HISTORY_CAP` and never count as unread.

A single bounded recall page (`RECALL_LIMIT = 100`) is loaded per login — the
reference's fixed recall window; the underlying library fully supports
multi-page paging (unit-tested), so
**older paging on scroll-to-top is a clean follow-up**.

Unit-tested (nearby paging in both `chat_log.rs` shells; the recall-above-live
render in `conversations.rs`). Live-verified on OpenSim: a fresh login's
Conversations → Nearby tab loaded the account's persisted `chat.txt` history
above the session's live lines (user-confirmed).

Follow-ups filed: [[viewer-chat-overlay-fade]] (the bottom-left `chat.rs`
overlay still keeps stale lines forever — make them decay); clickable links in
the transcript are the existing [[viewer-url-linkification]]; server-side group
/ session chat backlog is [[chat-group-history-server-side]].
