---
id: protocol-join-session-voice-posts-accept-invitation
title: Joining a session's voice posts accept invitation, where the reference posts call or start p2p voice
topic: protocol
status: bugs
origin: protocol-voice-accept-on-direct-session-becomes-conference (2026-11-18)
refs: [protocol-voice-accept-on-direct-session-becomes-conference, viewer-voice-call-dialogs]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

`Command::JoinSessionVoice` makes both runtimes POST `accept invitation` to
`ChatSessionRequest` for every kind of session (`sl-client-tokio/src/lib.rs`
and `sl-client-bevy/src/lib.rs`, the `JoinSessionVoice` arms). The reference
uses `accept invitation` only to answer a group or conference invitation
(`chatterBoxInvitationCoro`). To get a channel for a session's voice it posts
other methods:

- a group's or a conference's channel: `call`, with `alt_params` naming the
  voice server type (`LLVoiceChannelGroup::voiceCallCapCoro`,
  `llvoicechannel.cpp`);
- starting a 1:1 call: `start p2p voice`, with the other agent as `params`
  (`startP2PVoiceCoro`, `llimview.cpp`);
- answering an incoming 1:1 call: nothing (`processCallResponse`), which
  `Command::AcceptChatInvite` now does.

So joining the voice of a 1:1 conversation posts a request whose meaning on
Second Life for a P2P session id is unknown, and joining a group's voice
never asks for the channel the way the reference does.

## To do

- Read the three reference paths in full (the `alt_params` /
  `voice_server_type` members, and what each reply carries).
- Have `JoinSessionVoice` post `call` for a group or a conference and
  `start p2p voice` for a 1:1 call it starts, in both runtimes, with the
  fake grid's `ChatSessionRequest` answering both.
- Live on aditi once voice works ([[viewer-voice-call-dialogs]]).
