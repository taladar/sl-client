---
id: chat-group-history-server-side
title: Server-side group / session chat history ("fetch history")
topic: chat
status: ready
origin: user request (2026-07-21); follow-up to viewer-chat-history-panel
points: 5
refs: [chat-a8, chat-b4, viewer-chat-history-panel, viewer-social-im-conversations]
---

Context: [context/viewer.md](../context/viewer.md).

Second Life keeps a **server-side recent-message backlog for group (and
possibly ad-hoc conference) chat sessions** and hands it to a client that
opens or joins a session, so it sees what was said before it was listening.
Today our history is live-only (the [[chat-b4]] in-memory ring) or
client-persisted (our transcripts, [[viewer-chat-history-panel]]); offline-IM
retrieval ([[chat-a8]]) never carries group session chat. This task adds the
server backlog as a third source and the reference colour convention that
distinguishes all three on screen.

## Findings (reference research, 2026-07-22)

Verified in phoenix-firestorm; this is upstream LL code (~2023), not an FS
extension (only the colouring is FS-only).

- **Mechanism**: the existing `ChatSessionRequest` capability with
  `method: "fetch history"` — no dedicated cap, no event-queue history
  message. Request body `{ "method": "fetch history", "session-id": <uuid> }`
  (`llimview.cpp:792`, `chatterBoxHistoryCoro`).
- **Response**: an LLSD **array** (oldest→newest; bounded — the server
  decides how many recent messages to return) of maps
  `{ from: name string, from_id: uuid, message: string, time: int epoch,
  num/index: int }` (shapes documented at `llimview.cpp:835` and `:1370`).
- **Triggers** (gated on per-account setting `FetchGroupChatHistory`,
  default **on**): (1) a group message arriving that opens/creates the
  session — the triggering message is passed along for de-dup
  (`llimview.cpp:3672`); (2) `ChatterBoxSessionStartReply` success for a
  session you started/joined (`llimview.cpp:4906`).
- **Merge**: `LLIMSession::addMessagesFromServerHistory`
  (`llimview.cpp:1260`) splices the fetched backlog ahead of locally-cached
  and just-arrived lines, de-duplicated (the triggering message arrives both
  live and inside the fetch).
- **Not this feature**: `ChatterBoxInvitation` carries only the single
  triggering message; offline IM (`ReadOfflineMsgs` /
  `RetrieveInstantMessages`) stores p2p IMs, object IMs, group *notices* and
  invites but never `IM_SESSION_SEND` (dialog 17) — the reference drops group
  session messages when no session is open (`llimprocessing.cpp:1857`).
- **OpenSim implements none of this**: no `ChatSessionRequest` handler at all
  (only commented-out voice stubs), Groups V2 messaging is
  online-members-only, and both offline-IM modules exclude `SessionSend`
  from storage. **Live testing is aditi-only**; on OpenSim the fetch must
  degrade silently (cap present but unanswered never happens — the cap is
  simply absent).
- Whether ad-hoc conferences also return history is untested — observe on
  aditi and record here.

## Colour convention (reference, exact values)

The reference distinguishes three message classes in IM/group windows by hue
(`EChatStyle`, `llchat.h:69`; mapping in `fschathistory.cpp:1566-1582`):

- **Local transcript recall** — `CHAT_STYLE_HISTORY`, colour
  `ChatHistoryMessageFromLog` = `0.5 0.5 0.5 1` (mid grey).
- **Server-fetched history** — `CHAT_STYLE_SERVER_HISTORY` (FS-only style,
  `<FS:Zi>`), colour `ChatHistoryMessageFromServerLog` =
  `0.37 0.51 0.38 1` (muted green).
- **Live messages** — `CHAT_STYLE_NORMAL`, `AgentChatColor` → White
  `1 1 1 1`.

The Vintage skin overrides none of these names, so the values above are what
Vintage users see. Firestorm additionally fades *all* IM body text (live and
history) to `FSIMChatHistoryFade` = 0.5 alpha; the class distinction is by
hue, and we adopt the hues only (skip the global fade).

## Work

1. **sl-proto (sans-IO)**: `CHAT_SESSION_FETCH_HISTORY = "fetch history"`
   beside `CHAT_SESSION_ACCEPT`/`DECLINE` (`session.rs`); decoder
   `session_history_from_llsd` in `session/conversions.rs` (the array shape
   above, oldest-first; `time` may arrive int or real); new
   `Command::FetchSessionHistory { kind }` and
   `Event::SessionServerHistory { kind, messages }`. Store the backlog on
   `ChatSession` (`session/chat_session.rs`) in a bounded buffer **separate
   from the live `history` ring** so unread / `HISTORY_CAP` semantics stay
   untouched, de-duplicated against the ring by sender + text + timestamp
   slack (the triggering message arrives twice).
2. **Runtimes (tokio + bevy at parity, + REPL command)**: when a `Group` /
   `Conference` session reaches `Joined` (accept-invitation reply, or
   lazy-open on first inbound `SessionSend`), auto-issue the fetch if the cap
   is present (defer-until-cap convention), plus honour the explicit
   command. Reply routing: `post_chat_session_request`
   (`sl-client-tokio/src/http.rs`) tags replies as the roster-shaped accept
   reply — the fetch-history reply is an **array**, so tag it distinctly
   (e.g. synthetic cap name `"ChatSessionRequest/fetch history"` +
   session-id) and route it to the new decoder in `handle_caps_event`. Do
   **not** write server history into on-disk transcripts in the first cut
   (transcript = what this client heard live); revisit as a follow-up.
3. **Viewer (`conversations.rs`)**: render group/conference tabs in three
   bands — grey local recall, green server history (de-duped against recall
   by name + text), white live lines — mirroring the Nearby
   `recall.iter().chain(lines.iter())` pattern. Consume the
   already-implemented-but-unused `Command::QueryChatHistoryPage` /
   `Event::ChatHistoryPage` so keyed-session tabs get local recall at all
   (today only Nearby has it), and style existing recall lines grey per the
   convention. Colours as bevy_flair CSS tokens with the reference values;
   user toggle as a CLI arg (e.g. `--no-group-chat-history`, default on) per
   the CLI-options convention.
4. **Testing**: unit tests for the decoder, de-dup, and band rendering; live
   verification aditi-only — generate group traffic with the second account,
   relog the first, open the group tab, expect the green backlog. Confirm
   OpenSim (cap absent) shows nothing and logs no errors.
