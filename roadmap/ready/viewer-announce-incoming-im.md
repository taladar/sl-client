---
id: viewer-announce-incoming-im
title: Announce/auto-open on incoming IM typing
topic: viewer
status: ready
origin: debug-settings/chat-lines survey (2026-07-23)
refs: [chat-b3, viewer-social-im-conversations]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm can react to the *typing-start* of an inbound IM — before the
message even arrives — by opening the conversation and posting an
"X is typing an IM to you" notice, plus flashing the conversation entry
on new messages. Our typing tracking exists ([[chat-b3]], done) but is
only shown as a passive per-session indicator.

Scope:

- On inbound IM typing-start, optionally open (or surface) the IM
  session and post a system notice line (`FSAnnounceIncomingIM`).
- Conversation-list flash on new message / typing
  (`FSFlashOnMessage`, `FSNotifyIMFlash`) and optional flash on friend
  status change (`FSIMChatFlashOnFriendStatusChange`).
- All behaviours settings-gated and off by default, matching the
  reference defaults.

Reference (Firestorm, read-only): the `FSAnnounceIncomingIM` /
`FSFlashOnMessage` settings and `fsfloaterim*` consumers.

Builds on: participant typing tracking and the IM conversations UI (both
done).
