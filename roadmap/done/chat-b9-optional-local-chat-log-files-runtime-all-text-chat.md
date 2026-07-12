---
id: chat-b9
title: Optional local chat-log files (runtime, all text chat)
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B9. Optional local chat-log files (runtime, all text chat) — DONE 2026-06-27

*(was old B13, minus the `SessionMessage` rename already done in B4 — from
A13.)* After B4 (the entry type) and B7 (the `QueryChatHistoryPage` it extends).
Runtime only; the sans-IO `Session` does no file I/O. Grid-agnostic, testable
anywhere. See § Local chat-log files reference (from A13).

- [x] Add the `chat_log` core: `ChatLogConfig` (default off; per-type enable
      with nearby default off, log dir, filename scheme, timestamp format with
      **seconds default on**, recall window, `conversation.log` on/off +
      retention), `cleanFileName` sanitisation, the four filename schemes
      (optional date suffix, conference MD5 hash), and the Firestorm line
      `[YYYY/MM/DD HH:MM:SS]  Name: message` (local wall-clock, `\n␠`
      continuation, `%3A` colon-escape, `Second Life:` system name).
      **Refinement:** the **pure** half (config + format/parse + filenames)
      lives in `sl-proto::chat_log` (re-exported flat at the crate root); each
      runtime
      keeps only a thin file-I/O shell (`chat_log.rs`, `time` for the local
      clock, `fs_err` for the I/O). `Session` still does no file I/O. The config
      is enum/set-shaped, not a bool clutch (`enabled:
      BTreeSet<LoggedChatType>`, `timestamp: Option<TimestampFormat>`,
      `ClockStyle`), to satisfy the struct-bool lint.
- [x] Tap the event stream + own-outbound commands: on `ChatReceived`,
  `InstantMessageReceived` (dialog `Message`) / `GroupSessionMessage` /
  `ConferenceSessionMessage` and our `InstantMessage` / `SendGroupMessage` /
  `SendConferenceMessage`, write the line to the right file when that type's
  logging is enabled (tokio + bevy at parity). **Group-name cache:** group
  names are harvested from `GroupMemberships` / `GroupNames` /
  `GroupProfileReceived` / `ConferenceInvited`; a group message that arrives
  before its name is **buffered and flushed** once known, so a group file is
  never written under a raw UUID. 1:1 peer names come from the inbound
  `from_name` cache; the conference roster from `Session::participants`.
- [x] Extend B7's `QueryChatHistoryPage` runtime handler: when the cursor
      points past the in-memory tail, read the transcript, parse lines back into
      `SessionMessage`s (failed parse → plain-text fallback; **fold any
      non-timestamped / blank continuation line**, validated against the user's
      `sl-chat-log-parser`), return the page + older `prev`. **Refinement (user
      request):** on-demand paging reads the **whole** transcript so all history
      is reachable — it is *not* capped at `LOG_RECALL_SIZE` (kept only as a
      documented Firestorm-parity constant). `MessageCursor` gained
      `from_consumed` / `consumed_count` so the runtime can cross the
      memory→file boundary.
- [x] Optional `conversation.log` index: write/update per conversation, purge
  entries older than the retention days on load. Wire `ChatLogConfig` through
  each runtime's constructor at parity (`Client::set_chat_log_config` /
  `SlClientPlugin::chat_log_config`); the REPL exposes the toggles via a shared
  `sl_repl::ChatLogArgs` flattened into both binaries. **Refinement:** the
  index `offline`/unread flag is always clear (the shell does not track read
  state — the index is for conversation discovery); the `pid`/`sid` fields are
  filled best-effort (Direct: peer + canonical session id; Group/Conference:
  own id + group/session id).
- [x] Tests (run anywhere): a logged message writes the exact Firestorm line
      with seconds; each type maps to the right sanitised filename (incl. the
      conference MD5 + legacy-name option); a multi-line message round-trips
      through `\n␠` (plus internal blank lines and untimestamped
      continuations); read-back parses a stored line into a `SessionMessage`
      and a malformed line into a plain-text fallback; `QueryChatHistoryPage`
      past the tail returns file-backed pages and walks the whole history to
      exhaustion; the `conversation.log` line round-trips and stale entries
      purge on load; default-off writes nothing; a group message buffers until
      its name flushes it. (20 in `sl-proto`, 8 each in `sl-client-tokio` /
      `sl-client-bevy`.)
