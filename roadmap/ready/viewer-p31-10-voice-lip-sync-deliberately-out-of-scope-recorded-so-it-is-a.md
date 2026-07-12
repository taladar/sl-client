---
id: viewer-p31-10
title: Voice lip-sync — deliberately OUT OF SCOPE (recorded so it is a known gap, not an oversight)
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.10. Voice lip-sync — deliberately OUT OF SCOPE (recorded so it is
a known gap, not an oversight).** The reference viewer animates an avatar's
mouth from the live voice **audio power** while it speaks
(`LLVoiceVisualizer` — a viseme / mouth-open morph driven by the speaker's
amplitude), plus the green voice-dots "who's speaking" indicator. Both need
the decoded voice **audio stream**, which sl-client does not carry: the
project models voice **signalling / session-state only**, not the
Vivox / WebRTC audio transport, and the speaking indicators are out too (see
the voice-signalling-only decision in the sl-client memory). So there is
nothing to drive lips or dots from. Left unchecked as a permanent,
intentional boundary; revisit only if voice audio is ever brought in scope.
