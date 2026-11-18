---
id: protocol-voice-accept-on-direct-session-becomes-conference
title: A voice accept on a 1:1 session is folded into a phantom conference
topic: protocol
status: done
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

## Resolution (2026-11-18)

The runtimes no longer write anything into a `ChatSessionRequest` reply. Each
POST carries the session it is for, typed, to its reply: `ChatSessionReply {
session: ChatSessionKind, purpose, body }` (`purpose` an `Answer` or a
`FetchHistory`) comes back on a channel of its own in both runtimes and is
handed to the new `Session::handle_chat_session_reply`. The two arms of
`handle_caps_event` that rebuilt the kind from `session-id` / `from_group`,
the `CapsEvent::ChatSessionRequest` / `ChatSessionFetchHistory` tags and the
synthetic `CHAT_SESSION_FETCH_HISTORY_TAG` are gone. A voice accept on a 1:1
session now records the call's channel on that session, and opens no
conference beside it.

The `from_group` bools went with it. `Event::ConferenceInvited` names its
session as a `ChatSessionKind` (`session`), and `Command::AcceptChatInvite` /
`DeclineChatInvite` take it (`{ session }`); `Session::accept_chat_invite` /
`decline_chat_invite` and the REPL's `accept_chat_invite` /
`decline_chat_invite` (`<direct|group|conference> <id>`) likewise. The
viewer's test helper that read answers as `(id, from_group, accepted)` reads
`(ChatSessionKind, InviteAnswer)`.

The reference was the check for a 1:1 call, since a live call needs voice:

- A voice invitation states its kind in `voice.invitation_type`, which the
  decoder ignored: 2 is `P2P_CHAT_SESSION` (`EMultiAgentChatSessionType`,
  `llimview.cpp`), a 1:1 call the reference opens as a P2P session
  (`inviteToSession` with `IM_SESSION_P2P_INVITE`). Such an invitation is now
  the `Direct` session with the inviter, pending as an invitation there; the
  fake grid's encoder states the type for every voice invitation.
- The reference accepts a 1:1 call with **no** POST
  (`LLIncomingCallDialog::processCallResponse`: `addP2PSession`, then
  `startCall`) and declines one with `decline p2p voice`. Both runtimes now do
  the same; without the cap a declined call has nothing to send.
- Declining a call on a conversation already open with the caller keeps the
  conversation, without the offer.

Live on 2026-11-18, `chat-invite-accept-decline` passes on aditi (the accept
reply's roster routed by the typed kind) and on the local OpenSim (the UDP
`SessionLeave` fallback). Not verified live: a 1:1 voice accept on Second
Life, which needs working voice ([[viewer-voice-call-dialogs]]).

Follow-ups filed: [[protocol-join-session-voice-posts-accept-invitation]] (what
`Command::JoinSessionVoice` posts) and
[[protocol-group-fetch-history-403-on-aditi]] (seen in the aditi run, not
caused by this change).
