---
id: viewer-gst-audio-mixer-handoff
title: GStreamer audio into the shared mixer (parcel stream + video PCM)
topic: viewer
status: blocked
origin: follow-up filed while implementing viewer-video-playback /
  viewer-streaming-audio with the interim direct audio path (2026-07-22)
blocked_by: [viewer-audio-backend]
refs: [viewer-video-playback, viewer-streaming-audio, viewer-volume-panel]
---

Context: [context/viewer.md](../context/viewer.md).

Replace `sl-gst`'s interim direct audio output (`playbin3`'s default
`autoaudiosink`) with the shared mixer once [[viewer-audio-backend]]
exists. Two consumers, two buses:

- **Parcel radio stream** (`AudioStreamPlayer`,
  `sl-client-bevy-viewer/src/parcel_audio.rs`): swap the audio sink for an
  f32 `appsink` and push the PCM through the mixer's resampling channel
  (`fixed_resample::ResamplingChannel` — the stream's clock is not the
  sound card's) onto the **music bus, stereo, not spatialised**.
- **Media-on-a-prim video** (`GstMediaSurface` in `sl-gst/src/surface.rs`):
  same `appsink` hand-off, but onto the **media bus, spatialised at the
  prim** — the place we beat the reference viewer, whose plugin plays
  straight to the OS device with `setPan()` an empty stub.

The part that must be settled first (flagged in the original task): **A/V
sync and clock ownership**. Today both of GStreamer's sinks follow the
pipeline clock, which solves sync by accident. Once the mixer owns the
audio device, something must keep the video `appsink`'s frame delivery in
step with audio the mixer now schedules — either slave the pipeline clock
to the mixer's device clock, or run the audio `appsink` `sync=false` and
let the resampling channel's drift correction absorb the difference.
Decide before wiring, not after.

Volume / mute plumbing stays as-is externally: the per-surface
`set_volume` / `set_muted` and the persisted `MusicStreamVolume` setting
simply start driving mixer-bus gains instead of `playbin` properties (the
[[viewer-volume-panel]] reads the same buses).

Builds on: `sl-gst` (both players), `parcel_audio.rs`, `media_prim.rs` —
all shipped with the interim path and an API already shaped for this swap.
