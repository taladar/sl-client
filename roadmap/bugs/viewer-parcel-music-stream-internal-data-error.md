---
id: viewer-parcel-music-stream-internal-data-error
title: Parcel music stream fails to play ("Internal data stream error")
topic: viewer
status: bugs
origin: user report (2026-08-02, aditi live testing)
refs: [viewer-streaming-audio, viewer-music-controls-push-chat-bar]
---

Context: [context/viewer.md](../context/viewer.md).

## Symptom

On a parcel that advertises a music-stream URL, the parcel-audio cluster
appears and the stream is attempted (autoplay when `MusicStreamEnabled`, or the
play button), but nothing plays. The stream player logs:

```text
WARN sl_gst::stream: audio stream error: Internal data stream error.
```

Observed live on an aditi test region (2026-08-02) while verifying
[[viewer-music-controls-push-chat-bar]]; the error fired ~13 s after the region
handshake completed.

## What is *not* the cause (ruled out this run)

This is **not** obviously the known "dev box is missing GStreamer plugins"
situation. In the same run the video-media engine initialised and
`sl_gst::playback_gaps()` logged **no** `video-media capability gap` warnings —
so its probes for an HTTP(S) source (`souphttpsrc` / `gst-plugins-soup`), the
HLS demuxer (`hlsdemux2`), and the MP3 decoder all passed. (Contrast the earlier
note — in the `sl-client-media-test-grid-state` progress memory — that those
Gentoo ebuilds were missing as of 2026-07-22; they appear present now, or the
probes are satisfied another way.) So the missing-element story does not explain
this on its own.

`playback_gaps()` only checks that *elements exist*, not that a real pipeline
runs end to end, so the failure is downstream of element availability.

## Where to look / hypotheses

- `sl-gst/src/stream.rs` (`AudioStreamPlayer`): how the playback pipeline is
  built for a bare stream URL, and what surfaces as the `Internal data stream
  error` (a generic GStreamer error a source/demux raises when it cannot
  produce data). `friendly_error` (`sl-gst/src/messages.rs`) maps errors using
  the *pipeline's* discovered missing plugins — check whether a decoder that is
  needed only once data flows (e.g. a specific codec/container the probes did
  not cover) is missing at runtime.
- The actual stream URL from the aditi test parcel: is it reachable, is it an
  HLS `.m3u8` vs. a raw Icecast/Shoutcast MP3/AAC, does it need a redirect or a
  `User-Agent` `souphttpsrc` is not sending, is it `https` with a cert issue?
  Capture the exact `music_url` (`parcel_audio.rs` resolves it from
  `SlAgentParcel.current.music_url`) and try it directly with
  `gst-launch-1.0 playbin uri=<url>` to separate "the stream is dead" from "our
  pipeline is wrong".
- Compare against the reference viewer's `llstreamingaudio` /
  `llaudiodecodemgr` pipeline for the same URL to see whether it plays there.

## Verify

Re-test on a parcel with a *known-good* stream (ideally one the reference
viewer plays) so a dead upstream stream is not mistaken for a client bug, then
confirm audio actually plays and the now-playing title / state update.
