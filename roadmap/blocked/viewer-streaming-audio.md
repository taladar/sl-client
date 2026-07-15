---
id: viewer-streaming-audio
title: Parcel streaming-audio / media-audio player
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-audio-backend]
---

Context: [context/viewer.md](../context/viewer.md).

Play the parcel audio stream URL (Shoutcast / Icecast / HLS) and media-clip
audio, with a nearby-media control panel (play / stop / volume, autoplay
policy, per-parcel switching on region/parcel change).

The audio device, decode and mixer are **not** this task's problem — they belong
to [[viewer-audio-backend]]. What is specific here is the *network stream*, and
the 2026-07 research settles how to get one: **GStreamer**, which
[[viewer-video-playback]] pulls in anyway. `souphttpsrc iradio-mode=true !
icydemux` handles Shoutcast / Icecast **including ICY metadata** — that is the
"now playing" title a viewer shows — and HLS comes free via `adaptivedemux2`.

The pure-Rust audio crates genuinely have no story here: symphonia is a
demuxer/decoder, not a network stack (no ICY, no HLS, no reconnect), and rodio's
symphonia backend panics on non-seekable sources. Choosing them would mean
writing an Icecast client, an ICY de-interleaver, an HLS manifest parser and a
segment fetcher from scratch. Note the reference viewer does not hand-roll this
either — it hands the URL to FMOD and lets FMOD own the network stack. We hand
it to GStreamer instead.

So: GStreamer **decodes**, and pushes PCM into [[viewer-audio-backend]]'s mixer
through a resampling channel (the stream's clock is not the sound card's). Put
it on the **music bus as stereo — not spatialised**: parcel audio is ambient
music, not a positional source. Only media-on-a-prim audio is positional.

The parcel media / audio **protocol** is already done (`protocol-24`); this is
the playback + control surface on top: the parcel stream URL, per-parcel
switching on region / parcel change, the autoplay policy, and the nearby-media
control panel (play / stop / volume).

Reference (Firestorm, read-only): `llaudio/llstreamingaudio_*`,
`llviewermedia_streamingaudio`, `llpanelnearbymedia`, `llviewerparcelmedia`.

Builds on: `protocol-24` parcel media / audio caps. Supersedes the MVP "no
sound" non-goal.

Deps: [[viewer-audio-backend]] (device, decode, mixer).
