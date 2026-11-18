---
id: protocol-join-session-voice-provisions-vivox
title: Joining a session's voice posts a Vivox account provision
topic: protocol
status: bugs
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
