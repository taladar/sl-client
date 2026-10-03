---
id: gridspec-voice
title: Voice provisioning and signalling on each grid
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-voice-infrastructure,
  viewer-login-voice-config-unread]
blocked_by: [viewer-voice-audio]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

SL is WebRTC-only; stock OpenSim loads no voice module; the fake grid has a
WebRTC stub.

## Discover

`test-voice-account` / `test-voice-signaling` once the viewer can make a genuine
WebRTC offer.

## Document

`book/src/gridspec/voice.md`.

## Fake grid

Small — align the stub with the measured shapes.

## Viewer

Voice unavailable on OpenSim, no Vivox fallback.
