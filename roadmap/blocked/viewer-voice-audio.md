---
id: viewer-voice-audio
title: Voice audio transport (WebRTC — no Vivox)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-widget-scaffold, viewer-audio-backend]
---

Context: [context/viewer.md](../context/viewer.md).

Turn the existing voice **signalling** into actual talk / listen: modern SL
WebRTC media, microphone capture, per-speaker volume, and "who's speaking"
indicators. The UI over this transport is [[viewer-voice-controls]] and
[[viewer-voice-call-dialogs]].

**This explicitly supersedes the recorded "voice = signalling only" scope
decision** (superseded in memory 2026-07-22). **Vivox is out of scope
entirely** (user decision, 2026-07-22): even Linden Lab no longer uses it,
so nothing depending on the legacy Vivox path gets built — WebRTC only.

## The server spatialises, not us (2026-07 research)

The single most important correction to the obvious design: **SL's WebRTC voice
server is a mixing, spatialising server.** Per Linden Lab's own developer
documentation, the viewer opens an SCTP data channel named **`SLData`** (before
negotiation) and sends its *sender* position/heading **and the listener's**
position/heading as JSON; the server returns **one pre-spatialised stereo Opus
stream** per region, plus per-agent RMS and voice-activity for the speaking
indicators. Per-peer PCM never reaches us.

So voice is a **stereo, non-spatial bus** in the mixer — do *not* try to place
each speaker at their avatar (there is nothing to place). Per-avatar mute and
gain are free: they are just `SLData` messages. Cross-region means holding
**several simultaneous peer connections** (own region primary, neighbours
non-primary), playing audio from all and sending the mic only to the primary.

Also concrete from the same source: signalling is LLSD over CAPS
(`ProvisionVoiceAccountRequest` carrying the JSEP offer, `VoiceSignalingRequest`
for trickle ICE), and the SDP offer **must be munged** —
`minptime=10;useinbandfec=1;stereo=1;sprop-stereo=1;maxplaybackrate=48000`, i.e.
Opus, stereo, 48 kHz.

## Transport and the echo-cancellation trap

`webrtc` (webrtc-rs) is the likely pick — we need data channels *and* a full
peer connection with offer/answer — with `str0m` the alternative (its author
notes the p2p path is less tested). Neither decodes Opus; bring the `opus`
crate.

**Neither ships an audio processing module — verified: zero AEC in either.**
Without acoustic echo cancellation, remote participants hear themselves. AEC3
needs a **render reference** (what actually went to the speakers) alongside the
mic, on the same clock — which is precisely why [[viewer-audio-backend]] should
pick a mixer whose graph *contains* the mic (Firewheel's `AudioGraphInput`), so
an AEC node can see the post-mix output and the capture in one `process()` call.
A mixer with no audio input forces the mic onto a second device clock and leaves
us hand-rolling drift and delay estimation.

For the AEC itself: `webrtc-audio-processing` (bindings to the standalone
PulseAudio/PipeWire-maintained C++ library — battle-tested, builds from a
bundled submodule with no Chromium checkout, at the cost of a C++ toolchain), or
`sonora` (a pure-Rust port of the WebRTC APM — AEC3, noise suppression, AGC;
ideal for our build constraints but young and unproven). OS-level echo
cancellation is the fallback, but the Windows/macOS variants want to own both
capture and render, which fights our mixer.

Note nobody has driven the SL voice server from a non-libwebrtc client: the SDP
munging, DTLS fingerprints and `SLData` channel ordering are plausible with
webrtc-rs but **unproven**. Expect that to be the risk, not the audio graph.

Reference (Firestorm, read-only): `llwebrtc/`, `llvoicewebrtc`, `llvoicevivox`,
`llvoiceclient`, `llvoicechannel`, `llvoicevisualizer`,
`fsfloatervoicecontrols`.

Builds on: `protocol-26` voice signalling + `sl-client-bevy/src/voice.rs`.

Deps: [[viewer-ui-widget-scaffold]], [[viewer-audio-backend]] (the shared device
and mixer — and the mic capture that AEC depends on).
