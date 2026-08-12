---
id: test-chat-whisper-shout-range-aditi
title: Chat whisper/shout range — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `chat-whisper-shout-range` case
(`[[test-chat-whisper-shout-range]]`, already green `[opensim]`): **pass
(partial) on aditi live** (2026-08-12, Phase Z batch) after a real client
fix. The case separates the two avatars with intra-region teleports, but
**Second Life's parcel routing (landing point / telehub) silently
overrode even the anchor teleport**, landing the avatar tens of metres
from the requested position — so the whisper/shout separation the case
depends on is not achievable on that region and an out-of-range message
was heard. Fix: `Event::TeleportLocal` now carries the simulator's
authoritative landing position (previously discarded), and the case
compares it (horizontally, within a 2 m tolerance) against the request;
on an override it records a partial ("parcel routing overrode the
requested teleport positions") rather than asserting bogus chat ranges.
The whisper/shout range assertions themselves stay green on any region
that honours the positions (OpenSim, and SL parcels without a landing
point).
