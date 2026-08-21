---
id: protocol-sim-voice-signalling
title: Server-side voice provisioning and WebRTC signalling stub
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 5
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The server side of `ProvisionVoiceAccount`, `ParcelVoiceInfo` and
`VoiceSignalingRequest`: a signalling stub speaking the SDP/ICE exchange
shape already modelled client-side in `sl-wire/src/voice.rs`, sufficient
to drive the client's WebRTC voice path in tests (offer in → answer out,
ICE trickle events over the event queue).

The media plane itself stays out of the mock — this exercises the
client's signalling, not audio. Vivox remains out of scope entirely (LL
dropped it; the workspace is WebRTC-only for voice).

Done (2026-08-21): three `CapHandler` variants routed in `SimCaps`
(`sl-proto/src/sim_caps.rs`), coverage rows flipped to Served (57 granted
caps). The serving store is the new `SimVoice` stub
(`sl-proto/src/sim_voice.rs`, `SimSession::voice[_mut]`): a `WebRtcStub`
answerer whose `answer_for` derives a JSEP answer from the client's offer
(mirrored media sections, `a=setup:passive`, our ICE/DTLS identity and
candidates inline), an optional Vivox account fixture, a per-parcel
`ParcelVoiceInfo` table keyed to the agent's parcel, per-channel
credentials for `multiagent` sessions (`401` on mismatch, the viewer's
"channel locked"), and the live `VoiceConnection`s with their trickled
ICE candidates. `ServerEvent::{VoiceProvisionRequested,
VoiceSignalingReceived, ParcelVoiceInfoRequested}`.

Correction to the task text: the viewer has **no inbound ICE-trickle path**
— the server's candidates ride inside the synchronous JSEP answer and
`VoiceSignalingRequest` is client→server only — so there are no "ICE
trickle events over the event queue" to serve; the only voice EQ push is
the existing `RequiredVoiceVersion`, which the fake grid now sends on
arrival. Three codec gaps closed in `sl-wire`: `SimulatorFeatures
.voice_server_type` (`VoiceServerType`, how the viewer picks WebRTC), the
`channel` / `credentials` fields of the multi-agent
`VoiceProvisionRequest` (`webrtc_channel`), and `VoiceChannelUri`
(`Uri(sip:…)` | `Id(uuid)`) replacing the `url::Url` in `ParcelVoiceInfo`
/ `VoiceChannelInfo` — SL's WebRTC `channel_uri` is a bare region UUID,
which the old type rejected. `sl-fake-grid`'s stock scenario enables
WebRTC and advertises it (`voice-config`, `VoiceServerType`,
`RequiredVoiceVersion`). Verified by seven `sim_voice` unit tests, seven
loopback tests in `sl-proto/tests/sim_caps.rs` driving the real client
folds, and `voice_signalling_round_trips_through_the_real_client` in
`sl-fake-grid/tests/client_end_to_end.rs`; book coverage in "The voice
handlers" (`book/src/comms/caps.md`) and the fake-grid chapter.
