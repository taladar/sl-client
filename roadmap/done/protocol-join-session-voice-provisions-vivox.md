---
id: protocol-join-session-voice-provisions-vivox
title: Joining a session's voice posts a Vivox account provision
topic: protocol
status: done
origin: protocol-join-session-voice-posts-accept-invitation (2026-11-18)
refs: [protocol-join-session-voice-posts-accept-invitation, viewer-voice-audio]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

Both runtimes' `Command::JoinSessionVoice` arms still POST
`ProvisionVoiceAccountRequest` with `VoiceProvisionRequest::vivox()`
(`{ voice_server_type: "vivox" }`) before the session's channel is asked
for. Vivox is out of scope (WebRTC only), and the reference does not
provision on a join at all under WebRTC: it provisions a connection per
channel once it has the channel, as a `multiagent` provision carrying the
JSEP offer, the channel's `channel_uri` and its `channel_credentials`
(`LLVoiceWebRTCAdHocConnection`, `llvoicewebrtc.cpp`).

## To do

- Drop the Vivox provision from the join in both runtimes.
- Provision the session's channel the WebRTC way once
  `Session::session_voice_channel` holds one — which needs an offer, so it
  belongs with [[viewer-voice-audio]] or a signalling-level stand-in agreed
  there.

## Resolution (2026-11-19)

Both runtimes' `Command::JoinSessionVoice` arms no longer POST a provision:
the join only asks for the channel (`call` / `start p2p voice`) where it is
not known. The WebRTC provision is left to the engine that owns the offer,
through a signalling-level stand-in: `Session` reports a joined session's
channel as the new `Event::SessionVoiceChannelReady { session, channel }` — at
the join when the session already knows it (an invitation carried it), else
when a `call`'s `voice_credentials` or an accept's `voice_channel_info` brings
it — once per join, again only when the channel changes, never for a channel
with no id or a session whose voice is not joined (`ChatSession`'s
`voice_channel_announced`, cleared on leave / decline). The engine answers
with `Command::RequestVoiceAccount` and
`VoiceProvisionRequest::webrtc_channel(offer, channel, credentials)`, the
reference's `multiagent` body. Producing that offer is
[[viewer-voice-audio]], whose file now says so.

Tests: the group call, the 1:1 start then call and a known channel each
report the channel once; an accept's channel is reported only while joined,
not without an id, and again when it changes; leaving and rejoining a group
reports it again while an abandoned request reports nothing; and in
`sim_caps` the 1:1 call's reported channel, provisioned `multiagent` with its
credentials, connects to the fake simulator's WebRTC stub.

Live on aditi (`sl-repl-tokio`, primary, joining the census group's voice):
the join posted only `call`, the reply's WebRTC channel and credentials
surfaced as `session_voice_channel_ready`, and the trace holds no
`ProvisionVoiceAccountRequest` POST.
