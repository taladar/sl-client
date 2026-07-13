---
id: viewer-streaming-audio
title: Parcel streaming-audio / media-audio player
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Play the parcel audio stream URL (Shoutcast / Icecast / HLS) and media-clip
audio, with a nearby-media control panel (play / stop / volume, autoplay
policy, per-parcel switching on region/parcel change).

**Use a third-party audio backend — do not write a decoder / mixer.** First
fleshing-out step: survey and choose among candidate crates (`rodio`, `kira`,
`symphonia` + `cpal`, or a `gstreamer` / `libvlc` binding for HLS and awkward
codecs) on maturity, codec / stream coverage, latency, and Bevy fit; then
integrate.

The parcel media / audio **protocol** is already done (`protocol-24`); this is
the playback + control surface on top.

Reference (Firestorm, read-only): `llaudio/llstreamingaudio_*`,
`llviewermedia_streamingaudio`, `llpanelnearbymedia`, `llviewerparcelmedia`.

Builds on: `protocol-24` parcel media / audio caps. Supersedes the MVP "no
sound" non-goal.
