---
id: chat-group-history-server-side
title: Server-side group / session chat history retrieval
topic: chat
status: ideas
origin: user request (2026-07-21); follow-up to viewer-chat-history-panel
refs: [chat-a8, chat-b4, viewer-chat-history-panel]
---

Context: [context/viewer.md](../context/viewer.md).

Second Life appears to keep some **server-side history for group (and possibly
conference) chat sessions** — recent messages the grid can hand back so a client
that was offline, or that just joined the session, sees what was said before it
was listening, rather than only what arrived live on its own connection.

Today our chat history is either **live-only** (the sans-IO session's in-memory
ring, [[chat-b4]]) or **client-persisted** (our own on-disk transcripts, the
source [[viewer-chat-history-panel]] recalls for nearby chat). Neither pulls a
server-held backlog for a group session. Offline **IM** retrieval already exists
([[chat-a8]] — `RetrieveInstantMessages` / the `ReadOfflineMsgs` cap), but that
is one-to-one offline delivery, not a group-session scrollback.

Work:

- **Investigate first.** Confirm whether and how SL exposes server-side group /
  session chat history — the relevant capability or `ChatSessionRequest`
  operation, its request/response shape, retention window, and whether it
  applies to ad-hoc conferences too. Check the reference viewer (`llimview`,
  `llimhandler`, the `ChatSessionRequest` variants) and the OpenSim side for
  what, if anything, is implemented. OpenSim likely does **not** implement it,
  so live testing may be SL-only (aditi).
- If it exists, add the protocol/runtime path (a `Command` → cap request →
  `Event` carrying the recovered messages, mirroring the offline-IM and
  chat-history-page bridges) and merge it into a session's history ahead of the
  live ring, de-duplicated against what we already hold.
- Surface it in the Conversations floater's group / conference tabs the same way
  nearby chat recalls its on-disk transcript in [[viewer-chat-history-panel]].

If the investigation finds SL exposes no such server-side backlog, record that
and close this as `wont-do` (client-side transcript logging is then the only
group-history source).
