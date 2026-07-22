---
id: viewer-streaming-audio
title: Parcel streaming-audio / media-audio player
topic: viewer
status: in-progress
origin: reference-viewer feature-cluster survey (2026-07)
refs: [viewer-audio-backend]
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

Deps: [[viewer-audio-backend]] (device, decode, mixer) — for the *mixer
hand-off only* since the interim below unblocked playback itself.

## Progress (2026-07-22)

The stream player and its bottom-bar controls are implemented:

- **`sl-gst` `AudioStreamPlayer`**: an audio-only `playbin3` (video /
  subtitle streams deselected) per stream URL — GStreamer owns the network
  stack as planned, ICY metadata arrives as title tags ("now playing"),
  buffering messages hold/resume the pipeline, and failures are loud:
  `missing-plugin` descriptions (and the no-HTTP-source case) become the
  status error text. `playback_gaps()` logs absent system capabilities
  (HTTP source, HLS demux, MP3/AAC/H.264 decoders) once at startup.
- **Viewer `parcel_audio` module**: follows `SlAgentParcel`'s `music_url`
  per parcel / region change; the autoplay policy is the persisted
  `MusicStreamEnabled` setting (default on) with a per-URL user-stop
  memory (stopping one parcel's stream does not silence the next). Volume
  lives in the persisted `MusicStreamVolume` setting.
- **Bottom-bar cluster** (trailing side of the bottom area, shown only
  while the parcel has a stream): ♫ marker, width-capped now-playing /
  host / error text, play–stop and mute glyph buttons, and a volume
  slider bound to the settings store. Registered as a gallery specimen
  (`parcel-audio-bar`).

**Interim**: audio goes straight to the system device (`autoaudiosink`) —
the same interim as CEF page audio — because the mixer
([[viewer-audio-backend]]) does not exist yet. Still open here: the PCM
hand-off to the mixer's music bus (filed as
[[viewer-gst-audio-mixer-handoff]]), and the fuller
nearby-media panel (the reference's list of *all* nearby media with
per-item control) beyond this compact cluster.
