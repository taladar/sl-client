---
id: protocol-sim-voice-signalling
title: Server-side voice provisioning and WebRTC signalling stub
topic: protocol
status: ready
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
