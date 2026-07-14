---
id: viewer-streaming-audio
title: Parcel streaming-audio / media-audio player
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-audio-backend]
---

Context: [context/viewer.md](../context/viewer.md).

Play the parcel audio stream URL (Shoutcast / Icecast / HLS) and media-clip
audio, with a nearby-media control panel (play / stop / volume, autoplay
policy, per-parcel switching on region/parcel change).

The audio device, decode and mixer are **not** this task's problem — they belong
to [[viewer-audio-backend]], which owns the backend choice. What is specific
here is that a long-running network *stream* is a different demand from
one-shot clip playback: Shoutcast / Icecast / HLS coverage, reconnect and
buffering behaviour, and the awkward codecs may constrain that choice, so this
task's needs must be on the table when the root picks a crate — and if the
chosen backend cannot stream, this is where a second, stream-only library gets
justified.

The parcel media / audio **protocol** is already done (`protocol-24`); this is
the playback + control surface on top: the parcel stream URL, per-parcel
switching on region / parcel change, the autoplay policy, and the nearby-media
control panel (play / stop / volume).

Reference (Firestorm, read-only): `llaudio/llstreamingaudio_*`,
`llviewermedia_streamingaudio`, `llpanelnearbymedia`, `llviewerparcelmedia`.

Builds on: `protocol-24` parcel media / audio caps. Supersedes the MVP "no
sound" non-goal.

Deps: [[viewer-audio-backend]] (device, decode, mixer).
