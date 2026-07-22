---
id: viewer-p31-10
title: Voice lip-sync
topic: viewer
status: blocked
origin: VIEWER_ROADMAP.md — P31.10; re-scoped 2026-07-22 when voice audio
  came in scope for the viewer
blocked_by: [viewer-voice-audio]
refs: [viewer-voice-controls]
---

Context: [context/viewer.md](../context/viewer.md).

**Re-scoped (2026-07-22): in scope.** This task originally recorded voice
lip-sync as deliberately out of scope under the library-era
"voice = signalling only" decision; that decision is superseded — voice
audio is being built ([[viewer-voice-audio]], WebRTC only).

The feature: animate a speaking avatar's mouth from their live voice
level, as the reference's `LLVoiceVisualizer` does — the "Ooh" / "Aah"
morphs driven by the speaker's audio power with the reference's
attack/decay envelope, gated on the lip-sync-enabled setting. Our
per-speaker input is the WebRTC server's **per-agent RMS /
voice-activity** stream ([[viewer-voice-audio]] surfaces it — the same
data driving the speaking indicators in [[viewer-voice-controls]]);
per-speaker PCM never reaches the client, so amplitude-driven morphs
(not viseme analysis) are exactly what is possible — which matches what
the reference does anyway.

Builds on: the per-frame visual-param morph pipeline
(`viewer-p31-12a`) — the same driver the eye-blink and hand morphs use.

Reference (Firestorm, read-only): `llvoicevisualizer`,
`llvoavatar` (`idleUpdateLipSync`), the `LipSyncEnabled` settings.

Deps: [[viewer-voice-audio]] (the per-agent voice-activity data).
