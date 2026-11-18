---
id: protocol-join-session-voice-posts-accept-invitation
title: Joining a session's voice posts accept invitation, where the reference posts call or start p2p voice
topic: protocol
status: done
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

## Resolution (2026-11-18)

Reading the reference in full showed that under WebRTC, the only backend this
client speaks, there is no peer-to-peer call path: `getOutgoingCallInterface`
is null for WebRTC, so a 1:1 conversation's voice is an
`LLVoiceChannelGroup` with `mIsP2P`, started with `start p2p voice` and then,
once the session's `ChatterBoxSessionStartReply` arrives, asked for its
channel with `call` like any group. And `LLVoiceChannel::activate` asks only
from `STATE_NO_CHANNEL_INFO`: a session whose invitation carried a channel
posts nothing.

`Session::join_session_voice` now records that request per session
(`VoiceRequestState`): none when the channel is known, `call` for a group or a
conference, `start p2p voice` for a 1:1 call. Both runtimes POST what
`Session::next_session_voice_requests` returns, with the body and purpose from
`SessionVoiceRequest::body` / `purpose` (WebRTC named in `alt_params`, as
`preferred_voice_server_type` for a `call` and `voice_server_type` for a
start). A 1:1 start reply under the conversation's id makes the `call` due and
no longer opens a conference of that id. A `call`'s `voice_credentials`
become the session's channel. `ChatSessionReply` gained the HTTP `status`, and
the runtimes hand a refusal back even when its body is not LLSD, so a `403`
call (`NotAllowed`), another refusal, a call with no channel and a refused
start each end the join with the new `Event::SessionVoiceFailed`; a refused
accept or history fetch now changes nothing. Leaving a 1:1 call forgets its
channel, as the reference voids a P2P ad-hoc channel; leaving abandons a
request in flight. The fake grid answers `start p2p voice` and `call` in the
shapes measured on aditi.

Live on aditi with `sl-repl-tokio` probes (secondary avatar): a 1:1 call to
the primary went `start p2p voice` → start reply under the 1:1 id (`type` 2)
→ `call` → `voice_credentials` with a WebRTC channel (`invitation_type` 2); a
group's `call` was answered with its channel (`invitation_type` 0), leaving
posted `decline invitation`, and rejoining posted nothing. Connecting audio to
the channel is [[viewer-voice-audio]].
