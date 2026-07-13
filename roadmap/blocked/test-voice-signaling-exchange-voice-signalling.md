---
id: test-voice-signaling
title: exchange voice signalling
topic: test
status: blocked
origin: TEST_ROADMAP.md — Phase 17 — Voice signalling `[aditi] 1av`
blocked_by: [test-voice-account]
---

Context: [context/test.md](../context/test.md).

`voice-signaling` — exchange voice signalling. `1av`/`2av`.

**Blocked on [[test-voice-account]] (itself blocked on the viewer's
not-yet-built WebRTC voice engine).** The
`VoiceSignalingRequest` capability trickles WebRTC ICE candidates keyed by the
`viewer_session` from a prior WebRTC provision reply — both the candidates and
the session come from a real WebRTC peer connection this signalling-only client
does not embed (Second Life is WebRTC-only now; Vivox is gone and we will not
implement it). With no live session to key on there is nothing to exchange, so
this case waits on the viewer's WebRTC voice stack (planned late in the
timeline). The client-side signalling (`Command::SendVoiceSignaling`) and its
wire encoding already exist and are unit-tested in `sl-wire`; revisit this live
case once voice support lands. Depends on [[test-voice-account]].
