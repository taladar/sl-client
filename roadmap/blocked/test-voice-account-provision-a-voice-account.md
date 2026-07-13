---
id: test-voice-account
title: provision a voice account
topic: test
status: blocked
origin: TEST_ROADMAP.md — Phase 17 — Voice signalling `[aditi] 1av`
blocked_by: [viewer-voice-audio]
---

Context: [context/test.md](../context/test.md).

Signalling and session state only — no audio transport (out of scope).

`voice-account` — provision a voice account. `1av`.

**Blocked on [[viewer-voice-audio]]: needs the viewer's not-yet-built WebRTC
voice engine.** Second Life
voice is now **WebRTC-only** — Linden Lab removed Vivox completely, and we will
not implement Vivox. A WebRTC `ProvisionVoiceAccountRequest` carries a JSEP
**offer** SDP produced by a real WebRTC peer connection, which a signalling-only
client does not embed; the grid rejects a provision without a genuine offer, so
this case cannot round-trip until the viewer gains a WebRTC voice stack (planned
late in the timeline). The client-side `ProvisionVoiceAccountRequest` signalling
(`Command::RequestVoiceAccount` → `Event::VoiceAccountProvisioned`) already
exists and is unit-tested in `sl-wire`; revisit this live case once voice
support lands in the viewer.
