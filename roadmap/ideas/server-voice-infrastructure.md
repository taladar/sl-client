---
id: server-voice-infrastructure
title: Voice infrastructure — WebRTC media plane
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [protocol-sim-voice-signalling]
---

Context: [context/server.md](../context/server.md).

Real voice needs more than the signalling caps
([[protocol-sim-voice-signalling]] stubs those for tests): a WebRTC
media plane — an SFU or mixer terminating each agent's peer connection,
mixing/routing audio per parcel/estate voice channel and per private/
group call, applying **spatial audio** (position-driven gain/pan fed by
the simulator's agent positions), speaking indicators back through the
signalling path, and parcel voice permissions (voice-disabled parcels,
estate bans).

Candidate base: an existing Rust WebRTC stack (webrtc-rs; or wiring an
external SFU like LiveKit) — the build-vs-integrate call is the design
decision. Vivox is out of scope grid-wide (LL dropped it; this
workspace is WebRTC-only).
