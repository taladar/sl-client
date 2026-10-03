---
id: protocol-voice-accept-on-direct-session-becomes-conference
title: A voice accept on a 1:1 session is folded into a phantom conference
topic: protocol
status: bugs
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

The runtimes stamp a `ChatSessionRequest` reply with two invented LLSD keys,
`session-id` and `from_group` (`sl-client-bevy/src/http.rs`,
`sl-client-tokio/src/http.rs`), and the session rebuilds the kind as
`if from_group { Group } else { Conference }`
(`sl-proto/src/session/methods.rs`, the accept-roster and fetch-history arms).
`Command::JoinSessionVoice` on a **Direct** session sets
`from_group = matches!(.., Group)` = false (`sl-client-bevy/src/lib.rs`
~4231, `sl-client-tokio/src/lib.rs` ~2444).

## Effect

If the accept reply carries a roster or `voice_channel_info`, it lands on
`ChatSessionKind::Conference { id: <the 1:1 session id> }`; `chat_session_mut`
creates that phantom joined conference and the real Direct session never
records that voice was offered. A missing key silently becomes the nil uuid /
`false`. `Command::AcceptChatInvite` / `DeclineChatInvite { from_group }`,
`Event::ConferenceInvited { from_group }` and the viewer's
`invite_command(key, accept: bool) -> (session_id, from_group)` carry the
same lossy encoding.

## Fix

Carry `ChatSessionKind` with the request to its reply as a typed tag (the
runtime already owns the request), not as LLSD keys; replace the `from_group`
bools with the kind and `accept: bool` with an answer enum. Verify against
what Second Life returns for a P2P voice accept (aditi).
