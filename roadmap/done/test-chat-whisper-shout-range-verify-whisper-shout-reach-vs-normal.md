---
id: test-chat-whisper-shout-range
title: verify whisper/shout reach vs normal
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 2 — Local chat `[both]`
---

Context: [context/test.md](../context/test.md).

`chat-whisper-shout-range` — verify whisper/shout reach vs normal. `2av`
(OpenSim now; Aditi deferred → Phase Z). OpenSim drops an out-of-range message
outright (it never marks it less audible), so reach is simply whether the
relayed `ChatFromSimulator` arrives. The case anchors the primary and
teleports the secondary (an intra-region `Command::Teleport`, so the gap is
exact regardless of where each logged in) to two separations: at **15 m**
(between whisper's 10 m and say's 20 m) a normal say is heard but a whisper is
not, and at **60 m** (between say's 20 m and shout's 100 m) a shout is heard
but a say is not — establishing whisper < say < shout. At each gap the
secondary says the out-of-range message immediately followed by a louder
in-range sentinel; hearing the sentinel but never the out-of-range marker
(with a short grace against reordering) confirms the drop. Both avatars are
placed at a high Z so the teleport is not clamped to terrain and the
separation is purely horizontal. Green on OpenSim; say RTT ≈ 1–3 ms, shout
RTT ≈ 1 ms on loopback.
